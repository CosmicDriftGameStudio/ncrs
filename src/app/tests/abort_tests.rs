use crate::app::*;
use crate::fs::FileEntry;
use std::ffi::OsString;

fn app_with_files() -> App {
    let mut app = App::with_environment(Environment::isolated()).0;
    app.panels.left.path = PathBuf::from("/left");
    app.panels.right.path = PathBuf::from("/right");
    app.panels.left.entries = (0..3)
        .map(|i| FileEntry {
            name: OsString::from(format!("f{i}.txt")),
            path: PathBuf::from("/left").join(format!("f{i}.txt")),
            is_dir: false,
            is_symlink: false,
            is_parent: false,
            size: 1,
            modified: None,
            folder_size: None,
        })
        .collect();
    app.panels.left.selected = 0;
    app
}

/// Escape has to reach the app even though `route_key` looks at the
/// prompt: with none open it falls through to the bindings, and the abort
/// binding is what it has to find.
#[tokio::test]
async fn escape_stops_a_running_job() {
    let mut app = app_with_files();
    // Start a transfer, so there is something to stop.
    let _started = app.start_transfer(TransferKind::Copy);
    assert!(app.jobs.is_busy(), "nothing to abort");

    let routed = route_key(
        &app.prompt_key_state(),
        Key::Named(Named::Escape),
        Modifiers::default(),
    );
    let Some(message) = routed else {
        panic!("Escape while a job runs produced no message");
    };
    let _stopped = app.update(message);

    assert!(!app.jobs.is_busy(), "the job kept running after Escape");
    assert!(
        app.ops.transfer.is_none(),
        "the transfer was left half-done"
    );
}

/// After a stop, the next copy gets a channel somebody is listening on. A
/// sender kept from before the stop has no receiver, and every tick of the
/// next copy would fail on it.
#[tokio::test]
async fn a_copy_after_a_stop_gets_a_live_progress_channel() {
    let mut app = app_with_files();
    let _first = app.start_transfer(TransferKind::Copy);
    let _stopped = app.update(Message::AbortJob);

    let _second = app.start_transfer(TransferKind::Copy);

    let sender = app.progress_tx.as_ref().expect("a channel for the copy");
    assert!(!sender.is_closed(), "nobody listens to the new copy");
}

/// Stopping when nothing runs is harmless, not a panic.
#[test]
fn aborting_an_idle_app_does_nothing() {
    let mut app = app_with_files();
    let _task = app.update(Message::AbortJob);
    assert!(!app.jobs.is_busy());
}

/// The bar tells the user the job can be stopped, and stops claiming it
/// once nothing runs.
#[tokio::test]
async fn the_bar_mentions_the_abort_key_only_while_a_job_runs() {
    let mut app = app_with_files();
    assert!(app.job_status().is_none(), "an idle app shows a job");

    let _started = app.start_transfer(TransferKind::Copy);
    let status = app.job_status().expect("a running job should show");
    assert!(
        status.abortable,
        "a running job does not offer a way to stop it"
    );

    let _task = app.update(Message::AbortJob);
    assert!(app.job_status().is_none());
}
