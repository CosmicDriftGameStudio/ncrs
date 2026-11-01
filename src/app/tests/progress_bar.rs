use crate::app::*;
use crate::fs::FileEntry;
use std::ffi::OsString;

fn app_with_one_file() -> App {
    let mut app = App::with_environment(Environment::isolated()).0;
    app.panels.left.path = PathBuf::from("/left");
    app.panels.right.path = PathBuf::from("/right");
    app.panels.left.entries = vec![FileEntry {
        name: OsString::from("a.txt"),
        path: PathBuf::from("/left/a.txt"),
        is_dir: false,
        is_symlink: false,
        is_parent: false,
        size: 1,
        modified: None,
        folder_size: None,
    }];
    app.panels.left.selected = 0;
    app
}

/// A transfer of one file is one job, and the bar has to say so. The count
/// that matters is the one from the tick, not the number of rows: one
/// directory can hold fifty thousand files.
#[tokio::test]
async fn the_bar_shows_the_tick_not_the_row_count() {
    let mut app = app_with_one_file();
    let _started = app.start_transfer(TransferKind::Copy);
    assert!(app.jobs.is_busy(), "nothing to show progress for");

    // What the blocking thread would have sent: 1 of 1 for a single file.
    let _task = app.update(Message::JobProgress(fs::transfer::Tick {
        done: 1,
        total: 1,
    }));

    let status = app.job_status().expect("a running job should show");
    assert_eq!((status.done, status.total), (1, 1));
}

/// Before the first tick the bar must not claim to be finished: an unknown
/// total shows zero, not a full bar.
#[tokio::test]
async fn the_bar_does_not_claim_progress_before_the_first_tick() {
    let mut app = app_with_one_file();
    let _started = app.start_transfer(TransferKind::Copy);

    let status = app.job_status().expect("a running job should show");
    assert_eq!(status.done, 0, "the bar started at the end");
}

/// Stopping clears the progress, so the next job does not inherit a
/// previous one's numbers.
#[tokio::test]
async fn stopping_clears_the_progress() {
    let mut app = app_with_one_file();
    let _started = app.start_transfer(TransferKind::Copy);
    let _ticked = app.update(Message::JobProgress(fs::transfer::Tick {
        done: 5,
        total: 10,
    }));

    let _stopped = app.update(Message::AbortJob);
    assert!(
        app.job_status().is_none(),
        "the bar still shows a job that was stopped"
    );
}
