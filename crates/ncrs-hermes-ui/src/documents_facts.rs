//! What the Index and Hermes pages read from the index and the filing journal
//! in the background: the figures per read folder, the learned areas, the runs
//! and the line about the AI model.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Local, TimeZone as _};

use ncrs_ai::Compute;
use ncrs_config::config::FilingLocation;
use ncrs_core::facts::{AreaFacts, LoadedFacts, ReadFolderCoverage, RunDay, RunFacts};
use ncrs_filing::chat_work::{ChatAccess, ChatResult, ChatWork, RecentRun};
use ncrs_filing::filing::AiStatus;
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_index::BatchId;
use ncrs_intake::{area_order_plain, LocationMode};

/// The line under the AI row of the Index page: which model is in memory and
/// on what it computes, or why it failed.
pub fn ai_detail(status: &AiStatus, lang: Language) -> Option<String> {
    match status {
        AiStatus::Loaded {
            name,
            memory_bytes,
            compute,
        } => {
            let computes_on = match compute {
                Compute::Gpu(device) => device.as_str(),
                Compute::Cpu => lang.text(Msg::FilingStatusAiComputeCpu),
            };
            Some(fill_template(
                lang.text(Msg::FilingStatusAiLoaded),
                &[
                    ("{name}", name),
                    ("{memory}", &ncrs_ui_kit::format::size(*memory_bytes)),
                    ("{compute}", computes_on),
                ],
            ))
        }
        AiStatus::Failed(reason) => Some(fill_template(
            lang.text(Msg::FilingStatusAiFailed),
            &[("{reason}", reason)],
        )),
        AiStatus::Off
        | AiStatus::NotDownloaded
        | AiStatus::NotLoadedYet
        | AiStatus::Http { .. } => None,
    }
}

pub fn undo_batch(access: &ChatAccess, batch: i64, unexpected: String) -> Result<usize, String> {
    match access.run(ChatWork::Undo {
        batch: BatchId::from_number(batch),
    }) {
        ChatResult::Undone(undone) => undone.map(|report| report.left),
        _ => Err(unexpected),
    }
}

/// How many runs "Show all" can list.
const RUNS_LISTED_AT_MOST: usize = 50;

pub fn read_documents_facts(
    chat_access: Option<ChatAccess>,
    locations: &[FilingLocation],
    lang: Language,
    now: SystemTime,
) -> LoadedFacts {
    let disconnected: Vec<PathBuf> = locations
        .iter()
        .filter(|location| std::fs::metadata(&location.path).is_err())
        .map(|location| location.path.clone())
        .collect();
    let Some(access) = chat_access else {
        return LoadedFacts {
            disconnected,
            ..LoadedFacts::default()
        };
    };
    let mut areas = Vec::new();
    let mut folder_files = BTreeMap::new();
    let mut read_folders = ReadFolderCoverage::default();
    let mut error: Option<String> = None;
    for location in locations {
        match access.documents_under(&location.path) {
            Ok(files) => {
                if !disconnected.contains(&location.path) {
                    folder_files.insert(location.path.clone(), files);
                }
                // Counted from the index alone, so an unplugged drive keeps its numbers.
                if location.mode == LocationMode::Archive {
                    match access.ai_coverage_under(&location.path) {
                        Ok((with_text, understood)) => {
                            let wide = |count: usize| u64::try_from(count).unwrap_or(u64::MAX);
                            read_folders.documents += wide(files);
                            read_folders.with_text += wide(with_text);
                            read_folders.understood += wide(understood);
                        }
                        Err(failure) => {
                            error.get_or_insert(failure);
                        }
                    }
                }
            }
            Err(failure) => {
                error.get_or_insert(failure);
            }
        }
        let structure = match access.known_structure(&location.path) {
            Ok(structure) => structure,
            // Only a failed read is an error: a folder that is gone shows as disconnected,
            // and one that is not a folder or is too big has no structure to show.
            Err(ncrs_filing::chat_work::ReadError::Io(failure)) => {
                error.get_or_insert(failure);
                continue;
            }
            Err(_) => continue,
        };
        areas.extend(
            structure
                .areas
                .iter()
                .filter(|area| !area.folder.is_empty())
                .map(|area| AreaFacts {
                    name: area.folder.clone(),
                    files: usize::try_from(area.documents).unwrap_or(usize::MAX),
                    read_folder: location.path.clone(),
                    root: location.path.join(&area.folder),
                    order: area_order_plain(lang, area),
                }),
        );
    }
    let recent = access
        .recent_runs(RUNS_LISTED_AT_MOST)
        .unwrap_or_else(|failure| {
            error.get_or_insert(failure);
            Vec::new()
        });
    let runs = recent.iter().map(|run| run_facts(run, now)).collect();
    LoadedFacts {
        areas,
        runs,
        folder_files,
        read_folders: Some(read_folders),
        disconnected,
        error,
    }
}

fn folder_name(folder: &Path) -> String {
    folder
        .file_name()
        .unwrap_or(folder.as_os_str())
        .to_string_lossy()
        .into_owned()
}

fn run_facts(run: &RecentRun, now: SystemTime) -> RunFacts {
    let finished = Local
        .timestamp_opt(run.summary.finished_at, 0)
        .single()
        .unwrap_or_default();
    let (day, time) = run_day_and_time(finished, DateTime::<Local>::from(now));
    RunFacts {
        batch: run.summary.batch.number(),
        day,
        time,
        files: run.summary.items,
        from: run
            .summary
            .source_folder
            .as_deref()
            .map(folder_name)
            .unwrap_or_default(),
        to: run
            .target_folder
            .as_deref()
            .map(folder_name)
            .unwrap_or_default(),
        examples: run
            .examples
            .iter()
            .map(|example| example.to_string_lossy().into_owned())
            .collect(),
        undoable: run.undoable,
    }
}

/// Today and yesterday by the local calendar; older runs by their date.
fn run_day_and_time(finished: DateTime<Local>, now: DateTime<Local>) -> (RunDay, String) {
    let date = finished.date_naive();
    let today = now.date_naive();
    let day = if date == today {
        RunDay::Today
    } else if today.pred_opt() == Some(date) {
        RunDay::Yesterday
    } else {
        RunDay::On(date.format("%Y-%m-%d").to_string())
    };
    (day, finished.format("%H:%M").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn at(day: u32, hour: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 11, day, hour, 2, 0)
            .single()
            .expect("a valid local time")
    }

    #[test]
    fn a_run_is_today_yesterday_or_dated_by_the_local_calendar() {
        let now = at(4, 9);
        assert_eq!(
            run_day_and_time(at(4, 1), now),
            (RunDay::Today, "01:02".to_owned())
        );
        assert_eq!(run_day_and_time(at(3, 23), now).0, RunDay::Yesterday);
        assert_eq!(
            run_day_and_time(at(1, 14), now),
            (RunDay::On("2026-11-01".to_owned()), "14:02".to_owned())
        );
    }

    fn filing_location(path: PathBuf, mode: LocationMode) -> FilingLocation {
        FilingLocation {
            path,
            mode,
            images: false,
            is_target: false,
            transfer: ncrs_config::config::LocationTransfer::Move,
        }
    }

    fn index_document(
        index: &mut ncrs_index::Index,
        seed: u8,
        path: PathBuf,
        text: Option<&str>,
        understood: bool,
    ) {
        let id = index
            .insert_document(&ncrs_index::NewDocument {
                hash: ncrs_index::ContentHash([seed; 32]),
                path,
                source: ncrs_index::DocumentSource::Archive,
                received_at: chrono::Utc::now(),
                stamp: None,
            })
            .expect("insert");
        if let Some(body) = text {
            index.set_document_text(id, body).expect("text");
        }
        if understood {
            index.set_ai_read_version(id, 2).expect("asked");
        }
    }

    #[test]
    fn read_folder_coverage_counts_only_what_lies_below_read_folders() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (archive, inbox) = (dir.path().join("archive"), dir.path().join("inbox"));
        std::fs::create_dir_all(&archive).expect("archive");
        std::fs::create_dir_all(&inbox).expect("inbox");
        let mut index =
            ncrs_index::Index::create_plain(&dir.path().join("index.db")).expect("index");
        index_document(&mut index, 1, archive.join("read.pdf"), Some("Rent"), true);
        index_document(
            &mut index,
            2,
            archive.join("open.pdf"),
            Some("Letter"),
            false,
        );
        index_document(&mut index, 3, archive.join("scan.png"), None, false);
        index_document(
            &mut index,
            4,
            inbox.join("former-archive.pdf"),
            Some("Bill"),
            true,
        );
        let unplugged = dir.path().join("unplugged");
        index_document(&mut index, 5, unplugged.join("old.pdf"), Some("Tax"), true);
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        let access = ChatAccess::new(
            Arc::new(std::sync::Mutex::new(Some(index))),
            Arc::new(ncrs_intake::TextChain::new().with_source(ncrs_intake::PlainText)),
            Arc::new(ncrs_fs::JournaledTrash::new(
                dir.path().join("trash.toml"),
                ncrs_fs::FolderBackend(bin),
            )),
        );
        let locations = [
            filing_location(archive, LocationMode::Archive),
            filing_location(inbox, LocationMode::Inbox),
            filing_location(unplugged, LocationMode::Archive),
        ];
        let loaded = read_documents_facts(
            Some(access),
            &locations,
            Language::English,
            SystemTime::now(),
        );
        assert_eq!(
            loaded.read_folders,
            Some(ReadFolderCoverage {
                documents: 4,
                with_text: 3,
                understood: 2,
            })
        );
        assert_eq!(loaded.disconnected.len(), 1);
    }
}
