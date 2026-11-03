//! Waiting for the pipeline: every file in the index, then the AI through the
//! documents that have a text.

use std::time::{Duration, Instant};

use ncrs_filing::chat_work::AiCoverage;
use ncrs_index::{InMemoryKeyStore, Index};

use super::thread::documents_indexed;
use super::{expect, Protocol};
use crate::app::App;
use crate::messages::Message;

const AI_WAIT_VARIABLE: &str = "NCRS_PROTOCOL_AI_WAIT_SECS";
const AI_WAIT_DEFAULT: Duration = Duration::from_secs(3600);
const INDEX_STALL: Duration = Duration::from_secs(300);
const REPORT_EVERY: Duration = Duration::from_secs(10);
const PUMP_FOR: Duration = Duration::from_millis(500);

fn ai_wait_limit() -> Duration {
    std::env::var(AI_WAIT_VARIABLE)
        .ok()
        .and_then(|value| value.parse().ok())
        .map_or(AI_WAIT_DEFAULT, Duration::from_secs)
}

fn coverage_of(index: &Index, folder: &std::path::Path) -> AiCoverage {
    let (with_text, understood) = index.ai_coverage_beneath(folder).unwrap_or_default();
    AiCoverage {
        with_text,
        understood,
    }
}

impl<P: iced::Program<State = App, Message = Message> + 'static> Protocol<P> {
    /// Waits until the index holds every file and the AI has been through the
    /// documents with a text, or the time given for it ran out. Both go on
    /// side by side in the pipeline.
    pub(super) fn wait_for_indexing(&mut self) {
        let started = Instant::now();
        let limit = ai_wait_limit();
        let watcher = Index::open(&self.index_path, &InMemoryKeyStore::empty()).ok();
        let mut best = 0usize;
        let mut changed = Instant::now();
        let mut indexed_after = None;
        let mut coverage = AiCoverage::default();
        let mut last_report = Instant::now();
        loop {
            let rows =
                usize::try_from(documents_indexed(self.app()).unwrap_or(0)).unwrap_or(usize::MAX);
            // A file with the content of an indexed one is no new document.
            let duplicates = watcher.as_ref().map_or(0, |index| {
                [&self.archive, &self.downloads]
                    .into_iter()
                    .map(|folder| {
                        index
                            .duplicate_paths_under(folder)
                            .map_or(0, |found| found.len())
                    })
                    .sum()
            });
            let count = rows + duplicates;
            if count > best {
                best = count;
                changed = Instant::now();
            }
            let indexed = best >= self.expected_documents;
            if indexed && indexed_after.is_none() {
                indexed_after = Some(started.elapsed());
            }
            if let Some(index) = watcher.as_ref() {
                coverage = coverage_of(index, &self.archive);
            }
            if last_report.elapsed() >= REPORT_EVERY {
                last_report = Instant::now();
                println!(
                    "[{}] waiting: indexed {best}/{} understood {}/{} after {:.0}s",
                    self.language.code(),
                    self.expected_documents,
                    coverage.understood,
                    coverage.with_text,
                    started.elapsed().as_secs_f64()
                );
            }
            let ai_done = indexed && coverage.with_text > 0 && coverage.is_complete();
            let stalled = !indexed && changed.elapsed() > INDEX_STALL;
            if ai_done || stalled || started.elapsed() > limit {
                break;
            }
            self.pump(PUMP_FOR);
        }
        if let Some(index) = watcher.as_ref() {
            coverage = coverage_of(index, &self.archive);
            {
                std::fs::remove_file(&self.kept_index).ok();
                if let Err(error) = index.save_plain_copy(&self.kept_index) {
                    println!("the index could not be kept: {error}");
                }
            }
        }
        let indexing = indexed_after.unwrap_or_else(|| started.elapsed());
        self.record(
            "indexing",
            if self.reused {
                "started from the kept index, waited for the pipeline"
            } else {
                "waited for the pipeline to index the copy"
            },
            indexing.as_secs_f64(),
            "",
            "every file of the copy is in the index",
            expect(
                best >= self.expected_documents,
                format!(
                    "{best} of {} files indexed or known as duplicates",
                    self.expected_documents
                ),
            ),
        );
        let waited = started.elapsed().saturating_sub(indexing);
        self.record(
            "ai wait",
            &format!(
                "waited for the AI: {}/{} documents with text understood (limit {} s)",
                coverage.understood,
                coverage.with_text,
                limit.as_secs()
            ),
            waited.as_secs_f64(),
            "",
            "the AI has been through every document with a text",
            expect(
                best >= self.expected_documents && coverage.with_text > 0 && coverage.is_complete(),
                format!(
                    "{}/{} understood when the wait ended, {best} of {} files indexed",
                    coverage.understood, coverage.with_text, self.expected_documents
                ),
            ),
        );
    }
}
