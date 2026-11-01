use super::common::*;
use super::*;

#[derive(Default)]
struct Recorded(Mutex<Vec<fs::OpenCommand>>);

impl fs::Launcher for Recorded {
    fn launch(&self, command: &fs::OpenCommand) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|poisoned| poisoned.to_string())?
            .push(command.clone());
        Ok(())
    }
}

fn select(app: &mut App, side: PanelSide, name: &str) {
    let panel = app.panel_mut(side);
    panel.selected = panel.entries.iter().position(|e| e.name == name).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn f3_on_a_server_file_fetches_a_copy_and_hands_it_to_the_viewer() {
    let fixture = Fixture::connected().await;
    std::fs::write(
        fixture.server.local("/home/tester/notes.txt"),
        "from the server",
    )
    .unwrap();
    let cache = tempfile::tempdir().unwrap();
    let launcher = Arc::new(Recorded::default());
    let mut app = app_over(&fixture)
        .with_launcher(Arc::clone(&launcher) as Arc<dyn fs::Launcher>)
        .with_view_copies_in(&cache.path().join("view"));
    open(&mut app, PanelSide::Left, url("/home/tester"), None).await;
    select(&mut app, PanelSide::Left, "notes.txt");

    settle(&mut app, Message::OpenExternal(fs::OpenKind::View)).await;

    let launched = launcher.0.lock().unwrap().clone();
    assert_eq!(launched.len(), 1);
    let copy = PathBuf::from(launched[0].args.last().unwrap());
    assert_eq!(copy.file_name().unwrap(), "notes.txt");
    assert_eq!(std::fs::read_to_string(&copy).unwrap(), "from the server");
    assert!(copy.starts_with(cache.path()), "{copy:?}");
    assert!(!app.jobs.is_busy());
    assert!(app.ops.extracting.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_copy_goes_when_the_last_panel_leaves_the_server() {
    let fixture = Fixture::connected().await;
    std::fs::write(fixture.server.local("/home/tester/notes.txt"), "x").unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut app = app_over(&fixture)
        .with_launcher(Arc::new(Recorded::default()) as Arc<dyn fs::Launcher>)
        .with_view_copies_in(&cache.path().join("view"));
    open(&mut app, PanelSide::Left, url("/home/tester"), None).await;
    select(&mut app, PanelSide::Left, "notes.txt");
    settle(&mut app, Message::OpenExternal(fs::OpenKind::View)).await;
    let copies = || {
        walk(&cache.path().join("view"))
            .iter()
            .filter(|path| path.ends_with("notes.txt"))
            .count()
    };
    assert_eq!(copies(), 1);

    open(&mut app, PanelSide::Left, std::env::temp_dir(), None).await;

    assert_eq!(copies(), 0);
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walk(&path));
        }
        found.push(path);
    }
    found
}

#[tokio::test(flavor = "multi_thread")]
async fn f3_on_a_server_file_that_is_gone_says_why_and_leaves_no_copy() {
    let fixture = Fixture::connected().await;
    std::fs::write(fixture.server.local("/home/tester/gone.txt"), "x").unwrap();
    let cache = tempfile::tempdir().unwrap();
    let launcher = Arc::new(Recorded::default());
    let mut app = app_over(&fixture)
        .with_launcher(Arc::clone(&launcher) as Arc<dyn fs::Launcher>)
        .with_view_copies_in(&cache.path().join("view"));
    open(&mut app, PanelSide::Left, url("/home/tester"), None).await;
    select(&mut app, PanelSide::Left, "gone.txt");
    std::fs::remove_file(fixture.server.local("/home/tester/gone.txt")).unwrap();

    settle(&mut app, Message::OpenExternal(fs::OpenKind::View)).await;

    assert!(launcher.0.lock().unwrap().is_empty());
    assert!(app.status_error().is_some());
    assert!(walk(&cache.path().join("view"))
        .iter()
        .all(|p| !p.ends_with("gone.txt")));
}

#[tokio::test(flavor = "multi_thread")]
async fn f3_is_refused_while_another_job_runs() {
    let fixture = Fixture::connected().await;
    std::fs::write(fixture.server.local("/home/tester/a.txt"), "x").unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut app = app_over(&fixture)
        .with_launcher(Arc::new(Recorded::default()) as Arc<dyn fs::Launcher>)
        .with_view_copies_in(&cache.path().join("view"));
    open(&mut app, PanelSide::Left, url("/home/tester"), None).await;
    select(&mut app, PanelSide::Left, "a.txt");
    app.jobs.enqueue(jobs::Job {
        kind: jobs::JobKind::Copy,
        path: PathBuf::from("x"),
        total: None,
    });
    assert!(app.jobs.start_next(|_| Task::<Message>::none()).is_some());

    drop(app.update(Message::OpenExternal(fs::OpenKind::View)));

    assert!(app.ops.extracting.is_none());
    assert_eq!(
        app.notice_for_test(),
        Some("Another job is running; try again when it is done")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn f5_copies_a_server_file_to_the_other_panel_and_reloads_it() {
    let fixture = Fixture::connected().await;
    std::fs::write(fixture.server.local("/home/tester/doc.txt"), "downloaded").unwrap();
    let local = tempfile::tempdir().unwrap();
    let mut app = app_over(&fixture);
    open(&mut app, PanelSide::Left, url("/home/tester"), None).await;
    open(&mut app, PanelSide::Right, local.path().to_path_buf(), None).await;
    select(&mut app, PanelSide::Left, "doc.txt");

    let task = app.start_transfer(TransferKind::Copy);
    run(&mut app, task).await;

    assert_eq!(
        std::fs::read_to_string(local.path().join("doc.txt")).unwrap(),
        "downloaded"
    );
    assert!(app.ops.transfer.is_none());
    assert_eq!(app.status_error(), None);
    assert!(names(&app, PanelSide::Right).contains(&"doc.txt".to_owned()));
}

#[tokio::test(flavor = "multi_thread")]
async fn f5_copies_a_local_file_to_a_server_folder_and_reloads_it() {
    let fixture = Fixture::connected().await;
    let local = tempfile::tempdir().unwrap();
    std::fs::write(local.path().join("up.txt"), "uploaded").unwrap();
    let mut app = app_over(&fixture);
    open(&mut app, PanelSide::Left, local.path().to_path_buf(), None).await;
    open(&mut app, PanelSide::Right, url("/home/tester"), None).await;
    select(&mut app, PanelSide::Left, "up.txt");

    let task = app.start_transfer(TransferKind::Copy);
    run(&mut app, task).await;

    assert_eq!(
        std::fs::read_to_string(fixture.server.local("/home/tester/up.txt")).unwrap(),
        "uploaded"
    );
    assert_eq!(app.status_error(), None);
    assert!(names(&app, PanelSide::Right).contains(&"up.txt".to_owned()));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_name_taken_on_the_server_asks_and_overwrite_replaces_it() {
    let fixture = Fixture::connected().await;
    std::fs::write(fixture.server.local("/home/tester/same.txt"), "old").unwrap();
    let local = tempfile::tempdir().unwrap();
    std::fs::write(local.path().join("same.txt"), "new").unwrap();
    let mut app = app_over(&fixture);
    open(&mut app, PanelSide::Left, local.path().to_path_buf(), None).await;
    open(&mut app, PanelSide::Right, url("/home/tester"), None).await;
    select(&mut app, PanelSide::Left, "same.txt");

    let task = app.start_transfer(TransferKind::Copy);
    run(&mut app, task).await;
    assert!(
        app.ops.pending_conflict.is_some(),
        "the conflict dialog is asked"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.server.local("/home/tester/same.txt")).unwrap(),
        "old"
    );

    settle(
        &mut app,
        Message::TransferConflict(ConflictChoice::ThisOverwrite),
    )
    .await;

    assert_eq!(
        std::fs::read_to_string(fixture.server.local("/home/tester/same.txt")).unwrap(),
        "new"
    );
    assert!(app.ops.pending_conflict.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dropped_connection_during_a_copy_is_reported_not_hidden() {
    let fixture = Fixture::connected().await;
    std::fs::write(fixture.server.local("/home/tester/doc.txt"), "x").unwrap();
    let local = tempfile::tempdir().unwrap();
    let mut app = app_over(&fixture);
    open(&mut app, PanelSide::Left, url("/home/tester"), None).await;
    open(&mut app, PanelSide::Right, local.path().to_path_buf(), None).await;
    select(&mut app, PanelSide::Left, "doc.txt");
    fixture.pool.forget(&Fixture::id());

    let task = app.start_transfer(TransferKind::Copy);
    run(&mut app, task).await;

    assert!(app.status_error().is_some());
    assert!(!local.path().join("doc.txt").exists());
}

#[test]
fn move_to_or_from_a_server_is_refused_in_the_transfer_dialog() {
    let server_file = url("/home/tester/a.txt");
    let local_file = std::env::temp_dir().join("ncrs-never-read.txt");
    let local_folder = std::env::temp_dir();
    assert_eq!(
        transfer_dialog::validate(
            TransferKind::Move,
            std::slice::from_ref(&server_file),
            &local_folder,
        ),
        Err(transfer_dialog::Refusal::MoveFromServer)
    );
    assert_eq!(
        transfer_dialog::validate(TransferKind::Move, &[local_file], &url("/home/tester")),
        Err(transfer_dialog::Refusal::MoveToServer)
    );
}

#[test]
fn an_sftp_text_that_is_no_address_is_refused_and_never_looked_up_on_the_disk() {
    let local = vec![std::env::temp_dir().join("ncrs-never-read.txt")];
    for typed in [
        "sftp://host:99999/x",
        "SFTP://no-user.example.org/x",
        "sftp://u:secret@host/x",
        "sftp:// u@h/x",
        "sftp://",
    ] {
        let mut dialog = TransferDialog::new(1, TransferKind::Copy, local.clone(), url("/"));
        dialog.type_target(typed.to_owned());
        assert!(dialog.involves_server(), "{typed}: nothing is counted");
        assert_eq!(
            transfer_dialog::validate(TransferKind::Copy, &local, &dialog.target_path()),
            Err(transfer_dialog::Refusal::BadAddress),
            "{typed}"
        );
    }
    let mut dialog = TransferDialog::new(1, TransferKind::Copy, local.clone(), url("/"));
    dialog.type_target("sftp://tester@test.invalid/srv".to_owned());
    assert_eq!(
        transfer_dialog::validate(TransferKind::Copy, &local, &dialog.target_path()),
        Ok(url("/srv"))
    );
}

#[tokio::test]
async fn confirming_a_bad_address_shows_the_refusal_and_starts_nothing() {
    let fixture = Fixture::connected().await;
    let local = tempfile::tempdir().unwrap();
    std::fs::write(local.path().join("a.txt"), "x").unwrap();
    let mut app = app_over(&fixture);
    open(&mut app, PanelSide::Left, local.path().to_path_buf(), None).await;
    select(&mut app, PanelSide::Left, "a.txt");
    drop(app.update(Message::Transfer(TransferKind::Copy)));
    drop(
        app.update(Message::TransferDialog(TransferDialogMessage::TargetInput(
            "sftp://host:99999/x".to_owned(),
        ))),
    );

    drop(app.update(Message::TransferDialog(TransferDialogMessage::Confirm)));

    let dialog = app.ops.transfer_dialog.as_ref().unwrap();
    assert_eq!(dialog.refusal(), Some(transfer_dialog::Refusal::BadAddress));
    assert!(app.ops.transfer.is_none());
}

#[test]
fn server_to_server_copy_is_refused() {
    let result = transfer_dialog::validate(
        TransferKind::Copy,
        &[url("/home/tester/a.txt")],
        &RemotePath::new(ServerId::new("other", "elsewhere.invalid", 22), "/").to_path_buf(),
    );
    assert_eq!(result, Err(transfer_dialog::Refusal::ServerToServer));
}

#[test]
fn a_server_target_is_not_looked_up_on_the_disk() {
    let result = transfer_dialog::validate(
        TransferKind::Copy,
        &[std::env::temp_dir().join("ncrs-never-read.txt")],
        &url("/home/tester"),
    );
    assert_eq!(result, Ok(url("/home/tester")));
}

#[test]
fn the_target_typed_in_the_dialog_is_read_as_an_address_over_a_server() {
    let source = vec![std::env::temp_dir().join("a.txt")];
    let over_server = |typed: &str| {
        let mut dialog =
            TransferDialog::new(1, TransferKind::Copy, source.clone(), url("/home/tester"))
                .with_remote_home(Some(HOME.to_owned()));
        dialog.type_target(typed.to_owned());
        dialog.target_path()
    };
    assert_eq!(over_server("/home/tester"), url("/home/tester"));
    assert_eq!(over_server("sub"), url("/home/tester/sub"));
    assert_eq!(over_server(".."), url("/home"));
    assert_eq!(over_server("~/up"), url("/home/tester/up"));
    assert_eq!(
        over_server("sftp://tester@test.invalid:2222/x"),
        PathBuf::from("sftp://tester@test.invalid:2222/x")
    );
    let mut local = TransferDialog::new(1, TransferKind::Copy, source, std::env::temp_dir());
    local.type_target("sftp://tester@test.invalid/srv".to_owned());
    assert_eq!(local.target_path(), url("/srv"));
    assert!(local.involves_server());
}

#[tokio::test]
async fn nothing_is_counted_on_a_server_before_the_copy() {
    let fixture = Fixture::connected().await;
    let local = tempfile::tempdir().unwrap();
    let mut app = app_over(&fixture);
    open(&mut app, PanelSide::Left, url("/home"), None).await;
    open(&mut app, PanelSide::Right, local.path().to_path_buf(), None).await;
    select(&mut app, PanelSide::Left, "tester");

    let reported = reports(&mut app, Message::Transfer(TransferKind::Copy)).await;

    assert!(
        !reported.iter().any(|message| matches!(
            message,
            Message::TransferDialog(
                TransferDialogMessage::Scanned { .. }
                    | TransferDialogMessage::ConflictsCounted { .. }
            )
        )),
        "{reported:?}"
    );
    assert!(app.ops.transfer_dialog.as_ref().unwrap().counts_unknown());
}

#[tokio::test]
async fn confirming_a_move_from_a_server_shows_the_refusal_and_keeps_the_dialog() {
    let fixture = Fixture::connected().await;
    let local = tempfile::tempdir().unwrap();
    let mut app = app_over(&fixture);
    open(&mut app, PanelSide::Left, url("/home"), None).await;
    open(&mut app, PanelSide::Right, local.path().to_path_buf(), None).await;
    select(&mut app, PanelSide::Left, "tester");
    drop(app.update(Message::Transfer(TransferKind::Copy)));
    drop(
        app.update(Message::TransferDialog(TransferDialogMessage::SetKind(
            TransferKind::Move,
        ))),
    );

    drop(app.update(Message::TransferDialog(TransferDialogMessage::Confirm)));

    let dialog = app.ops.transfer_dialog.as_ref().unwrap();
    assert_eq!(
        dialog.refusal(),
        Some(transfer_dialog::Refusal::MoveFromServer)
    );
    assert!(app.ops.transfer.is_none());
}
