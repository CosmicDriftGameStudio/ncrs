//! The blocking work of the tab, started on the blocking pool.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use iced::Task;
use ncrs_git::RepoSummary;

use crate::bulk::{self, BulkAction, Executed, Outcome};
use crate::message::Message;
use crate::scan::{self, ScanRequest};
use crate::summary::{self, Reading};

pub fn read(path: PathBuf, stop: Arc<AtomicBool>) -> Task<Message> {
    let cleanable = crate::cleanup_tasks::cleanable(path.clone(), Arc::clone(&stop));
    let summary = Task::perform(
        async move {
            let to_read = path.clone();
            let reading = tokio::task::spawn_blocking(move || summary::read(&to_read, &stop))
                .await
                .unwrap_or_else(|error| Reading::Unreadable(error.to_string()));
            (path, reading)
        },
        |(root, reading)| Message::SummaryRead(root, Box::new(reading)),
    );
    Task::batch([summary, cleanable])
}

pub fn scan(request: ScanRequest) -> Task<Message> {
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || scan::scan(&request))
                .await
                .unwrap_or_default()
        },
        |result| Message::Scanned(Box::new(result)),
    )
}

pub fn step(
    action: BulkAction,
    path: PathBuf,
    before: RepoSummary,
    is_worktree: bool,
    stop: Arc<AtomicBool>,
) -> Task<Message> {
    Task::perform(
        async move {
            let to_run = path.clone();
            let executed = tokio::task::spawn_blocking(move || {
                bulk::execute(action, &to_run, &before, is_worktree, &stop)
            })
            .await
            .unwrap_or_else(|error| Executed {
                outcome: Outcome::Failed {
                    last_line: error.to_string(),
                },
                ran: Vec::new(),
                after: None,
            });
            (path, executed)
        },
        |(root, executed)| Message::StepDone(root, Box::new(executed)),
    )
}
