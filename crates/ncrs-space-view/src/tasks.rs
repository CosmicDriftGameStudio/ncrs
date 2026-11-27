//! The blocking work of the tab, started on the blocking pool.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use iced::futures::SinkExt as _;
use iced::Task;
use ncrs_core::FolderSize;
use ncrs_space::decide::{DownloadGroups, DownloadItem, SystemInstalledApps};
use ncrs_space::{Os, ScanRequest, SystemProbes};

use crate::confirm::RunPlan;
use crate::decide::DecideMessage;
use crate::message::{Finished, Message};

/// How many reports wait for the UI before further ones are dropped.
const REPORTS_KEPT: usize = 4;
/// How many run log files the log view reads: the newest.
const LOG_FILES_READ: usize = 3;

/// The measure of the drive: progress as bytes counted, then the scan; `None` when stopped.
pub fn measure(run: u64, request: ScanRequest, stop: Arc<AtomicBool>) -> Task<Message> {
    Task::run(
        iced::stream::channel(REPORTS_KEPT, async move |mut output| {
            let mut progress = output.clone();
            let done = tokio::task::spawn_blocking(move || {
                ncrs_space::scan(&request, &SystemProbes, &stop, |bytes| {
                    // A full channel drops a report: the next one is larger anyway.
                    drop(progress.try_send(Message::ScanProgress { run, bytes }));
                })
            })
            .await;
            let scan = done.ok().flatten().map(Arc::new);
            drop(output.send(Message::Scanned { run, scan }).await);
        }),
        std::convert::identity,
    )
}

pub fn request_of(ctx: &crate::ctx::Ctx) -> ScanRequest {
    ScanRequest {
        os: Os::current(),
        home: ctx.home.clone(),
        repos: ctx.repos.clone(),
        settings: ctx.settings.clone(),
    }
}

/// The delete run: the rows one after the other, then the worktrees; the log file at the end.
pub fn delete(
    run: u64,
    plan: RunPlan,
    home: PathBuf,
    log_dir: Option<PathBuf>,
    stop: Arc<AtomicBool>,
) -> Task<Message> {
    Task::run(
        iced::stream::channel(REPORTS_KEPT, async move |mut output| {
            let mut progress = output.clone();
            let done = tokio::task::spawn_blocking(move || {
                let safe_count = plan.safe.len();
                let mut done_rows = 0_usize;
                let mut log = ncrs_space::delete_safe(
                    &plan.safe,
                    &SystemProbes,
                    plan.unknown,
                    &stop,
                    |_row| {
                        done_rows += 1;
                        drop(progress.try_send(Message::RunProgress {
                            run,
                            done: done_rows,
                        }));
                    },
                );
                if !plan.worktrees.is_empty() && !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let removed =
                        ncrs_space::remove_worktrees(&plan.worktrees, &SystemProbes, plan.unknown);
                    log.finished = removed.finished;
                    log.rows.extend(removed.rows);
                    drop(progress.try_send(Message::RunProgress {
                        run,
                        done: safe_count + plan.worktrees.len(),
                    }));
                }
                let log_error = log_dir
                    .as_deref()
                    .and_then(|dir| ncrs_space::append_log(dir, &log).err())
                    .map(|error| error.to_string());
                let free_after = ncrs_os_probe::volume_space(&home)
                    .ok()
                    .map(|space| space.available);
                Finished {
                    run,
                    log: Arc::new(log),
                    free_after,
                    log_error,
                }
            })
            .await;
            if let Ok(finished) = done {
                drop(output.send(Message::RunFinished(finished)).await);
            }
        }),
        std::convert::identity,
    )
}

/// The Downloads folder sorted by reason, or the items of a big folder with their sizes.
pub fn load_decide(folder: PathBuf, is_downloads: bool, home: PathBuf) -> Task<Message> {
    Task::perform(
        async move {
            let read = folder.clone();
            let groups = tokio::task::spawn_blocking(move || {
                let never = AtomicBool::new(false);
                if is_downloads {
                    ncrs_space::decide::sort_downloads(
                        &read,
                        &SystemInstalledApps::new(&home),
                        &never,
                    )
                } else {
                    children_of(&read, &never)
                }
            })
            .await
            .unwrap_or_default();
            (folder, groups)
        },
        |(folder, groups)| {
            Message::Decide(DecideMessage::Loaded {
                folder,
                groups: Box::new(groups),
            })
        },
    )
}

/// What lies directly in a folder, biggest first, as one group; links and unreadable items are left out.
fn children_of(folder: &Path, stop: &AtomicBool) -> DownloadGroups {
    let Ok(listing) = std::fs::read_dir(folder) else {
        return DownloadGroups::default();
    };
    let mut items: Vec<DownloadItem> = listing
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path).ok()?;
            if metadata.file_type().is_symlink() {
                return None;
            }
            let size = if metadata.is_dir() {
                ncrs_fs::measure_folder(&path, stop).map_or(0, |size: FolderSize| size.bytes)
            } else {
                metadata.len()
            };
            let modified = metadata.modified().ok();
            Some(DownloadItem {
                path,
                size,
                modified,
                touched: modified,
                is_folder: metadata.is_dir(),
            })
        })
        .collect();
    items.sort_by(|left, right| right.size.cmp(&left.size));
    DownloadGroups {
        rest: items,
        ..DownloadGroups::default()
    }
}

/// The rows of the newest run log files, oldest first; the header lines of a run are left out.
pub fn read_log(dir: Option<PathBuf>) -> Task<Message> {
    Task::perform(
        async move {
            let Some(dir) = dir else {
                return Vec::new();
            };
            tokio::task::spawn_blocking(move || log_rows(&dir))
                .await
                .unwrap_or_default()
        },
        Message::LogRead,
    )
}

fn log_rows(dir: &Path) -> Vec<String> {
    let Ok(listing) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = listing
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("space-") && name.ends_with(".log"))
        })
        .collect();
    files.sort();
    let newest = files.len().saturating_sub(LOG_FILES_READ);
    files
        .iter()
        .skip(newest)
        .filter_map(|file| std::fs::read_to_string(file).ok())
        .flat_map(|text| {
            text.lines()
                .filter(|line| !line.starts_with('#') && !line.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect()
}
