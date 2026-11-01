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
