//! What Hermes tells in one line: the state of the pipeline, how many files
//! are in work, open, waiting for the user or failed. The status bar and the
//! title chip both read it, so they never disagree.

use ncrs_index::HermesCounts;
use ncrs_pipeline::{RunnerState, Snapshot, Task};

use ncrs_i18n::fill_template;
use ncrs_i18n::{Language, Msg};
use ncrs_ui_kit::format;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateWord {
    Running,
    Paused,
    /// Running, with nothing waiting and no worker busy on a step.
    Idle,
}

/// How the title chip is coloured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Fine,
    Paused,
    Stuck,
    Failing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HermesStatus {
    pub word: StateWord,
    /// Workers busy on a step.
    pub working: usize,
    /// Files queued at a step.
    pub open: usize,
    /// Open review entries.
    pub to_review: usize,
    /// Files that failed a step.
    pub failed: usize,
    pub pdf_library_missing: bool,
    pub stuck: bool,
}

/// Hermes is stuck when it is paused with files waiting, or when documents
/// wait for the user while it has not filed a single inbox document: it
/// cannot get on without the user then.
fn is_stuck(word: StateWord, open: usize, to_review: u64, inbox_filed: u64) -> bool {
    (word == StateWord::Paused && open > 0) || (to_review > 0 && inbox_filed == 0)
}

impl HermesStatus {
    /// `None` while the pipeline is off.
    pub fn of(
        snapshot: &Snapshot,
        counts: Option<&HermesCounts>,
        pdf_library_missing: bool,
    ) -> Option<Self> {
        if snapshot.state == RunnerState::Off {
            return None;
        }
        let working = snapshot
            .workers
            .iter()
            .filter(|worker| {
                worker
                    .busy
                    .as_ref()
                    .is_some_and(|busy| matches!(busy.task, Task::Step(_)))
            })
            .count();
        let open: usize = snapshot.steps.iter().map(|step| step.waiting).sum();
        let failed: usize = snapshot.steps.iter().map(|step| step.failed).sum();
        let word = if snapshot.state == RunnerState::Paused {
            StateWord::Paused
        } else if working == 0 && open == 0 {
            StateWord::Idle
        } else {
            StateWord::Running
        };
        let to_review = counts.map_or(0, |known| known.open_reviews);
        let inbox_filed = counts.map_or(1, |known| known.inbox.filed);
        Some(Self {
            word,
            working,
            open,
            to_review: usize::try_from(to_review).unwrap_or(usize::MAX),
            failed,
            pdf_library_missing,
            stuck: is_stuck(word, open, to_review, inbox_filed),
        })
    }

    /// The documents-per-minute rate only means something while files are
    /// in work or queued; it also counts documents routed to review, so an
    /// idle Hermes must not advertise it.
    pub fn is_busy(&self) -> bool {
        self.working > 0 || self.open > 0
    }

    pub fn is_failing(&self) -> bool {
        self.pdf_library_missing || self.failed > 0
    }

    pub fn tone(&self) -> Tone {
        if self.is_failing() {
            Tone::Failing
        } else if self.stuck {
            Tone::Stuck
        } else if self.word == StateWord::Paused {
            Tone::Paused
        } else {
            Tone::Fine
        }
    }

    /// The line of the status bar: the state word and the parts that are not
    /// zero; one notice instead when the PDF library is missing.
    pub fn line(&self, lang: Language) -> String {
        if self.pdf_library_missing {
            return lang.text(Msg::HermesStatusPdfMissing).to_owned();
        }
        let word = lang.text(match self.word {
            StateWord::Running => Msg::HermesStatusRunning,
            StateWord::Paused => Msg::HermesStatusPaused,
            StateWord::Idle => Msg::HermesStatusIdle,
        });
        let parts = [
            (self.working, Msg::HermesStatusWorking),
            (self.open, Msg::HermesStatusOpen),
            (self.to_review, Msg::HermesStatusToReview),
            (self.failed, Msg::HermesStatusFailed),
        ]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, template)| {
            fill_template(
                lang.text(template),
                &[("{count}", &format::grouped(count as u64))],
            )
        });
        std::iter::once(word.to_owned())
            .chain(parts)
            .collect::<Vec<_>>()
            .join(" \u{b7} ")
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use std::path::PathBuf;
    use std::time::SystemTime;

    use ncrs_index::StatusCounts;
    use ncrs_pipeline::{Busy, StepSnapshot, WorkerSnapshot};

    use super::*;

    fn snapshot(state: RunnerState, waiting: usize, failed: usize, busy: usize) -> Snapshot {
        let mut snapshot = Snapshot::off();
        snapshot.state = state;
        snapshot.steps = vec![StepSnapshot {
            name: "stamp",
            waiting,
            done: 0,
            failed,
            unchanged: 0,
        }];
        snapshot.workers = (0..busy)
            .map(|id| WorkerSnapshot {
                id,
                busy: Some(Busy {
                    file: PathBuf::from("a.pdf"),
                    task: Task::Step("stamp"),
                    started_at: SystemTime::UNIX_EPOCH,
                }),
            })
            .collect();
        snapshot
    }

    fn counts(open_reviews: u64, inbox_filed: u64) -> HermesCounts {
        HermesCounts {
            inbox: StatusCounts {
                filed: inbox_filed,
                ..StatusCounts::default()
            },
            open_reviews,
            ..HermesCounts::default()
        }
    }

    fn line(status: HermesStatus, lang: Language) -> String {
        status.line(lang)
    }

    #[test]
    fn nothing_is_said_while_the_pipeline_is_off() {
        assert!(HermesStatus::of(&Snapshot::off(), None, false).is_none());
    }

    #[test]
    fn a_running_pipeline_names_what_is_not_zero() {
        let status = HermesStatus::of(
            &snapshot(RunnerState::Running, 5, 0, 2),
            Some(&counts(3, 9)),
            false,
        )
        .unwrap();
        assert_eq!(
            line(status, Language::English),
            "running \u{b7} 2 working \u{b7} 5 open \u{b7} 3 to review"
        );
        assert_eq!(
            line(status, Language::German),
            "l\u{e4}uft \u{b7} 2 in Arbeit \u{b7} 5 offen \u{b7} 3 R\u{fc}ckfragen"
        );
        assert_eq!(status.tone(), Tone::Fine);
    }

    #[test]
    fn nothing_waiting_and_no_worker_busy_is_idle() {
        let status = HermesStatus::of(
            &snapshot(RunnerState::Running, 0, 0, 0),
            Some(&counts(0, 1)),
            false,
        )
        .unwrap();
        assert_eq!(status.word, StateWord::Idle);
        assert_eq!(line(status, Language::English), "idle");
        assert_eq!(line(status, Language::German), "ruht");
    }

    #[test]
    fn errors_show_up_and_turn_the_chip_red() {
        let status = HermesStatus::of(
            &snapshot(RunnerState::Running, 0, 4, 0),
            Some(&counts(0, 1)),
            false,
        )
        .unwrap();
        assert_eq!(line(status, Language::English), "idle \u{b7} 4 failed");
        assert_eq!(status.tone(), Tone::Failing);
    }

    #[test]
    fn a_missing_pdf_library_is_one_notice_and_red() {
        let status = HermesStatus::of(
            &snapshot(RunnerState::Running, 5, 0, 2),
            Some(&counts(3, 0)),
            true,
        )
        .unwrap();
        assert_eq!(
            line(status, Language::English),
            "PDF library missing, PDFs are not read"
        );
        assert_eq!(
            line(status, Language::German),
            "PDF-Bibliothek fehlt, PDFs werden nicht gelesen"
        );
        assert_eq!(status.tone(), Tone::Failing);
    }

    #[test]
    fn paused_with_files_waiting_is_stuck_and_paused_alone_is_not() {
        let waiting = HermesStatus::of(
            &snapshot(RunnerState::Paused, 3, 0, 0),
            Some(&counts(0, 1)),
            false,
        )
        .unwrap();
        assert_eq!(line(waiting, Language::English), "paused \u{b7} 3 open");
        assert_eq!(waiting.tone(), Tone::Stuck);
        let empty = HermesStatus::of(
            &snapshot(RunnerState::Paused, 0, 0, 0),
            Some(&counts(0, 1)),
            false,
        )
        .unwrap();
        assert_eq!(empty.tone(), Tone::Paused);
    }

    #[test]
    fn reviews_without_a_single_filed_document_are_stuck() {
        assert!(is_stuck(StateWord::Running, 0, 2, 0));
        assert!(!is_stuck(StateWord::Running, 0, 2, 1));
        assert!(!is_stuck(StateWord::Running, 0, 0, 0));
    }

    #[test]
    fn big_numbers_are_grouped() {
        let status = HermesStatus::of(
            &snapshot(RunnerState::Running, 0, 0, 0),
            Some(&counts(2421, 0)),
            false,
        )
        .unwrap();
        assert_eq!(
            line(status, Language::German),
            "ruht \u{b7} 2\u{a0}421 R\u{fc}ckfragen"
        );
    }
}
