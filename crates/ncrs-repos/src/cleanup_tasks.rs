//! The blocking work of the clean-up, started on the blocking pool.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use iced::Task;
use ncrs_git::{cleanup_list, refs, remote_tips, CleanupPlan, Git, UndoRecord};

use crate::bulk::error_line;
use crate::cleanup::{CleanupMessage, Scan};
use crate::message::Message;

/// What a listed repository has that can be cleaned up, from the refs as they
/// are (no fetch). Remote branches that were pruned count only once the pane
/// has proven them merged.
pub fn cleanable(path: PathBuf, stop: Arc<AtomicBool>) -> Task<Message> {
    Task::perform(
        async move {
            let to_read = path.clone();
            let count = tokio::task::spawn_blocking(move || {
                let overview = refs(&to_read, &stop).ok()?;
                Some(cleanup_list(&overview, &BTreeMap::new()).cleanable_count())
            })
            .await
            .ok()
            .flatten();
            (path, count)
        },
        |(root, count)| Message::CleanableRead(root, count),
    )
}

fn scan_blocking(path: &PathBuf, stop: &Arc<AtomicBool>) -> Result<Scan, String> {
    let git = Git::open_unattended(path).map_err(|error| error_line(&error))?;
    let tips_before = remote_tips(path, stop).unwrap_or_default();
    let fetch_error = git.fetch(None).err().map(|error| error_line(&error));
    let overview = refs(path, stop).map_err(|error| error.to_string())?;
    Ok(Scan {
        list: cleanup_list(&overview, &tips_before),
        default_branch: overview.main_branch,
        fetch_error,
    })
}

pub fn scan(path: PathBuf, stop: Arc<AtomicBool>) -> Task<Message> {
    Task::perform(
        async move {
            let to_scan = path.clone();
            let result = tokio::task::spawn_blocking(move || scan_blocking(&to_scan, &stop))
                .await
                .unwrap_or_else(|error| Err(error.to_string()));
            (path, result)
        },
        |(root, result)| Message::Cleanup(CleanupMessage::Scanned(root, Box::new(result))),
    )
}

pub fn delete(path: PathBuf, plan: CleanupPlan) -> Task<Message> {
    Task::perform(
        async move {
            let to_clean = path.clone();
            let result = tokio::task::spawn_blocking(move || {
                Git::open(&to_clean)
                    .and_then(|git| git.clean_up(&plan))
                    .map_err(|error| error_line(&error))
            })
            .await
            .unwrap_or_else(|error| Err(error.to_string()));
            (path, result)
        },
        |(root, result)| Message::Cleanup(CleanupMessage::Deleted(root, Box::new(result))),
    )
}

pub fn undo(records: Vec<(PathBuf, UndoRecord)>) -> Task<Message> {
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                records
                    .into_iter()
                    .map(|(path, record)| {
                        let result = Git::open(&path)
                            .and_then(|git| git.undo(&record))
                            .map_err(|error| error_line(&error));
                        (path, result)
                    })
                    .collect::<Vec<_>>()
            })
            .await
            .unwrap_or_default()
        },
        |results| Message::Cleanup(CleanupMessage::Undone(results)),
    )
}
