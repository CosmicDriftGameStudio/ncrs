use crate::app::*;

fn copy_path(app: &mut App) {
    drop(app.update(Message::CopyToClipboard(ClipboardKind::Path)));
}

#[test]
fn the_notice_goes_when_its_own_timer_fires() {
    let mut app = App::with_fixed_panels();
    copy_path(&mut app);
    assert_eq!(app.notice.as_deref(), Some("Path copied"));
    drop(app.update(Message::NoticeExpired(app.notice_generation)));
    assert_eq!(app.notice, None);
}

#[test]
fn an_older_timer_leaves_a_newer_notice_alone() {
    let mut app = App::with_fixed_panels();
    copy_path(&mut app);
    let first = app.notice_generation;
    drop(app.update(Message::CopyToClipboard(ClipboardKind::Name)));
    assert_ne!(app.notice_generation, first);

    drop(app.update(Message::NoticeExpired(first)));
    assert_eq!(app.notice.as_deref(), Some("Name copied"));

    drop(app.update(Message::NoticeExpired(app.notice_generation)));
    assert_eq!(app.notice, None);
}

#[test]
fn a_config_warning_stays_while_the_notice_comes_and_goes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[theme]\npanel = \"#f4f0ff\"\n").unwrap();
    let mut app = App::with_fixed_panels().with_config(config::load(&path));
    assert!(app.status_error().is_some());
    copy_path(&mut app);
    assert_eq!(app.notice.as_deref(), Some("Path copied"));
    assert!(app.status_error().is_some());
    drop(app.update(Message::NoticeExpired(app.notice_generation)));
    assert_eq!(app.notice, None);
    assert!(app.status_error().is_some());
}

#[test]
fn a_failure_replaces_the_notice() {
    let mut app = App::with_fixed_panels();
    copy_path(&mut app);
    app.set_status_error("it failed");
    assert_eq!(app.notice, None);
    assert_eq!(app.job_error.as_deref(), Some("it failed"));
}

fn run_a_job(app: &mut App) {
    app.jobs.enqueue(jobs::Job {
        kind: jobs::JobKind::Extract,
        path: PathBuf::from("/x/a.zip"),
        total: None,
    });
    assert!(app.jobs.start_next(|_| Task::<Message>::none()).is_some());
}

#[test]
fn a_job_asked_for_while_one_runs_gets_a_notice_and_is_not_queued() {
    let mut app = App::with_fixed_panels();
    run_a_job(&mut app);
    app.notice = None;

    drop(app.start_job(
        jobs::Job {
            kind: jobs::JobKind::Copy,
            path: PathBuf::from("/x/b.txt"),
            total: None,
        },
        Task::<Message>::none,
    ));

    assert_eq!(
        app.notice.as_deref(),
        Some(ncrs_ops::delete_flow::busy_text(app.lang).as_str())
    );
    assert!(app.jobs.is_busy());
    assert!(app.jobs.start_next(|_| Task::<Message>::none()).is_none());
}

#[test]
fn quitting_while_a_job_runs_opens_a_question_that_keeps_or_stops_it() {
    let mut app = App::with_fixed_panels();
    run_a_job(&mut app);

    drop(app.update(Message::Quit));
    assert!(app.ops.quit_dialog);
    assert_eq!(app.dialog_focus, ncrs_ops::quit_dialog::CANCEL_FOCUS);
    assert!(app.jobs.is_busy());

    drop(app.update(Message::from(ncrs_ops::OpsMessage::QuitCancel)));
    assert!(!app.ops.quit_dialog);
    assert!(app.jobs.is_busy(), "keeping the job stopped it");

    drop(app.update(Message::Quit));
    drop(app.update(Message::QuitConfirm));
    assert!(!app.ops.quit_dialog);
}

#[test]
fn quitting_with_a_note_that_cannot_be_saved_warns_once() {
    let scratch = tempfile::tempdir().unwrap();
    let blocker = scratch.path().join("blocker");
    std::fs::write(&blocker, b"a file, not a folder").unwrap();
    let mut app = App::with_fixed_panels();
    app.notes.unsaved.push(ncrs_notes::state::Write {
        path: blocker.join("note.md"),
        text: "unsaved".to_owned(),
        expect: ncrs_notes::store::Expect::Absent,
        replaces: None,
        generation: 1,
        root: scratch.path().to_owned(),
        background: false,
    });

    drop(app.update(Message::Quit));
    assert!(app.notes.quit_warned);
    assert!(
        app.job_error_for_test().is_some(),
        "the first quit did not warn"
    );

    app.job_error = None;
    drop(app.update(Message::Quit));
    assert_eq!(
        app.job_error_for_test(),
        None,
        "the second quit warned again"
    );
}

fn unsaveable_note(app: &mut App, scratch: &std::path::Path) {
    let blocker = scratch.join("blocker");
    std::fs::write(&blocker, b"a file, not a folder").unwrap();
    app.notes.unsaved.push(ncrs_notes::state::Write {
        path: blocker.join("note.md"),
        text: "unsaved".to_owned(),
        expect: ncrs_notes::store::Expect::Absent,
        replaces: None,
        generation: 1,
        root: scratch.to_owned(),
        background: false,
    });
}

#[test]
fn a_quit_held_back_for_a_note_leaves_the_job_running() {
    let scratch = tempfile::tempdir().unwrap();
    let mut app = App::with_fixed_panels();
    run_a_job(&mut app);
    unsaveable_note(&mut app, scratch.path());

    drop(app.update(Message::Quit));
    drop(app.update(Message::QuitConfirm));

    assert!(app.jobs.is_busy(), "the job was stopped, the app stayed");
    assert!(!app.ops.quit_confirmed);
}

#[test]
fn a_confirmed_quit_does_not_outlive_the_next_job() {
    let mut app = App::with_fixed_panels();
    app.ops.quit_confirmed = true;

    drop(app.start_job(
        jobs::Job {
            kind: jobs::JobKind::Copy,
            path: PathBuf::from("/x/b.txt"),
            total: None,
        },
        Task::<Message>::none,
    ));
    drop(app.update(Message::Quit));

    assert!(app.ops.quit_dialog, "the quit went through unasked");
}
