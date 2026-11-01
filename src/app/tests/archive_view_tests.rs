use crate::app::*;
use crate::fs::archive::fixtures::{scratch, write_archive, Item};
use crate::fs::{FileEntry, SortKey};
use std::ffi::OsString;

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

fn app_with_copies(root: &Path) -> App {
    App::with_environment(Environment::isolated())
        .0
        .with_launcher(Arc::new(Recorded::default()) as Arc<dyn fs::Launcher>)
        .with_view_copies_in(root)
}

fn leave(app: &mut App, side: PanelSide) {
    let request_id = app.panel_mut(side).begin_load();
    drop(app.update(Message::Panel(PanelMessage::DirectoryLoaded {
        side,
        request_id,
        path: PathBuf::from("/"),
        entries: Vec::new(),
        error: None,
        sorted_by: SortKey::default(),
        select: None,
        archive: None,
    })));
}

/// The state F3 leaves behind while its job runs, without a thread: the
/// slot exists, the job is the running one.
fn pretend_extracting(app: &mut App, archive: &Path) -> PathBuf {
    let slot = app.ops.view_copies.as_mut().unwrap().slot(archive).unwrap();
    app.ops.extracting = Some(Extracting {
        archive: archive.to_path_buf(),
        slot: slot.clone(),
    });
    app.jobs.enqueue(jobs::Job {
        kind: jobs::JobKind::Extract,
        path: archive.to_path_buf(),
        total: None,
    });
    assert!(app.jobs.start_next(|_| Task::<Message>::none()).is_some());
    slot
}

/// A panel standing in the zip `name` in `dir`, the cursor on its `a.txt`.
fn app_in_zip(dir: &Path, copies: Option<&Path>) -> (App, Arc<Recorded>) {
    let archive = dir.join("v.zip");
    write_archive(&archive, &[Item::File("a.txt", b"inside")]);
    let launcher = Arc::new(Recorded::default());
    let mut app = App::with_environment(Environment::isolated())
        .0
        .with_launcher(Arc::clone(&launcher) as Arc<dyn fs::Launcher>);
    if let Some(root) = copies {
        app = app.with_view_copies_in(root);
    }
    let panel = app.panel_mut(PanelSide::Left);
    panel.path = archive.clone();
    panel.archive = Some(archive.clone());
    panel.entries = vec![FileEntry {
        name: OsString::from("a.txt"),
        path: archive.join("a.txt"),
        is_dir: false,
        is_symlink: false,
        is_parent: false,
        size: 6,
        modified: None,
        folder_size: None,
    }];
    panel.selected = 0;
    (app, launcher)
}

#[tokio::test]
async fn the_copies_go_when_the_last_panel_leaves_the_archive() {
    let dir = scratch("view-leave");
    let archive = dir.path().join("x.zip");
    let mut app = app_with_copies(&dir.path().join("cache"));
    let slot = app
        .ops
        .view_copies
        .as_mut()
        .unwrap()
        .slot(&archive)
        .unwrap();
    app.panel_mut(PanelSide::Left).archive = Some(archive.clone());
    app.panel_mut(PanelSide::Right).archive = Some(archive.clone());

    leave(&mut app, PanelSide::Left);
    assert!(slot.exists(), "the other panel is still in the archive");

    leave(&mut app, PanelSide::Right);
    assert!(!slot.exists());
}

#[tokio::test]
async fn quitting_removes_every_copy_and_the_folder_of_the_run() {
    let dir = scratch("view-quit");
    let cache = dir.path().join("cache");
    let archive = dir.path().join("x.zip");
    let mut app = app_with_copies(&cache);
    let slot = app
        .ops
        .view_copies
        .as_mut()
        .unwrap()
        .slot(&archive)
        .unwrap();
    std::fs::write(slot.join("a.txt"), "x").unwrap();

    drop(app.update(Message::Quit));

    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
}

#[tokio::test]
async fn the_copies_of_another_archive_stay() {
    let dir = scratch("view-leave-other");
    let (here, there) = (dir.path().join("x.zip"), dir.path().join("y.zip"));
    let mut app = app_with_copies(&dir.path().join("cache"));
    let of_there = app.ops.view_copies.as_mut().unwrap().slot(&there).unwrap();
    app.panel_mut(PanelSide::Left).archive = Some(here);

    leave(&mut app, PanelSide::Left);

    assert!(of_there.exists());
}

#[tokio::test]
async fn leaving_the_archive_stops_its_unpacking() {
    let dir = scratch("view-leave-running");
    let archive = dir.path().join("x.zip");
    let mut app = app_with_copies(&dir.path().join("cache"));
    let slot = pretend_extracting(&mut app, &archive);
    let generation = app.ops.generation;
    app.panel_mut(PanelSide::Left).archive = Some(archive);

    leave(&mut app, PanelSide::Left);

    assert!(app.ops.extracting.is_none());
    assert!(!app.jobs.is_busy(), "the job is still running");
    assert_ne!(
        app.ops.generation, generation,
        "a late result would be taken"
    );
    assert!(!slot.exists());
}

#[tokio::test]
async fn leaving_another_archive_leaves_the_unpacking_alone() {
    let dir = scratch("view-leave-running-other");
    let (here, there) = (dir.path().join("x.zip"), dir.path().join("y.zip"));
    let mut app = app_with_copies(&dir.path().join("cache"));
    let slot = pretend_extracting(&mut app, &there);
    app.panel_mut(PanelSide::Left).archive = Some(here);

    leave(&mut app, PanelSide::Left);

    assert!(app.ops.extracting.is_some());
    assert!(app.jobs.is_busy());
    assert!(slot.exists());
}

#[tokio::test]
async fn a_busy_queue_starts_no_unpacking() {
    let dir = scratch("view-busy");
    let (mut app, launcher) = app_in_zip(dir.path(), Some(&dir.path().join("cache")));
    app.jobs.enqueue(jobs::Job {
        kind: jobs::JobKind::Copy,
        path: PathBuf::new(),
        total: None,
    });

    drop(app.update(Message::OpenExternal(fs::OpenKind::View)));

    assert_eq!(
        app.notice.as_deref(),
        Some(app.lang.text(Msg::StatusArchiveViewBusy))
    );
    assert!(app.ops.extracting.is_none());
    assert_eq!(app.jobs.waiting(), 1, "a job was queued behind the copy");
    assert!(launcher.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_copy_started_while_a_file_is_unpacked_is_refused_and_the_queue_survives() {
    let dir = scratch("view-copy-during");
    let archive = dir.path().join("v.zip");
    let (mut app, _launcher) = app_in_zip(dir.path(), Some(&dir.path().join("cache")));
    let slot = pretend_extracting(&mut app, &archive);

    drop(app.update(Message::Transfer(TransferKind::Copy)));

    assert!(app.ops.transfer_dialog.is_none(), "the dialog opened");
    assert_eq!(
        app.notice.as_deref(),
        Some(app.lang.text(Msg::StatusArchiveViewBusy))
    );
    assert!(app.jobs.is_busy(), "the unpacking was dropped");
    assert!(app.ops.extracting.is_some());

    let copy = slot.join("a.txt");
    std::fs::write(&copy, "x").unwrap();
    drop(app.update(Message::ViewExtracted {
        slot,
        result: Ok(copy),
        generation: app.ops.generation,
    }));

    assert!(!app.jobs.is_busy(), "the queue is stuck");
    assert!(app.ops.extracting.is_none());
}

/// Puts `app` in the state `name` says: something else owns the queue.
fn make_busy_with(name: &str, app: &mut App, folder: &Path) {
    let file = folder.join("s.txt");
    let transfer = Transfer {
        kind: TransferKind::Copy,
        sources: vec![file.clone()],
        target: folder.to_path_buf(),
        names: Vec::new(),
    };
    let deletion = Deletion {
        sources: vec![file.clone()],
        permanent: false,
        side: PanelSide::Left,
    };
    match name {
        "transfer" => app.ops.transfer = Some(transfer),
        "deleting" => app.ops.deleting = Some(deletion),
        "delete dialog" => app.ops.delete_dialog = Some(deletion),
        "transfer dialog" => {
            app.ops.transfer_dialog = Some(TransferDialog::new(
                1,
                TransferKind::Copy,
                vec![file],
                folder.to_path_buf(),
            ));
        }
        _ => app.ops.pending_conflict = Some(PendingConflict { transfer, index: 0 }),
    }
}

#[tokio::test]
async fn a_view_does_not_start_beside_a_transfer_a_delete_or_their_dialogs() {
    for name in [
        "transfer",
        "deleting",
        "delete dialog",
        "transfer dialog",
        "conflict",
    ] {
        let dir = scratch("view-beside");
        let (mut app, launcher) = app_in_zip(dir.path(), Some(&dir.path().join("cache")));
        make_busy_with(name, &mut app, dir.path());

        drop(app.update(Message::OpenExternal(fs::OpenKind::View)));

        assert_eq!(
            app.notice.as_deref(),
            Some(app.lang.text(Msg::StatusArchiveViewBusy)),
            "{name}"
        );
        assert!(app.ops.extracting.is_none(), "{name}");
        assert!(!app.jobs.is_busy(), "{name}");
        assert!(launcher.0.lock().unwrap().is_empty(), "{name}");
    }
}

#[tokio::test]
async fn without_a_cache_folder_the_view_says_so() {
    let dir = scratch("view-no-cache");
    let (mut app, launcher) = app_in_zip(dir.path(), None);

    drop(app.update(Message::OpenExternal(fs::OpenKind::View)));

    assert_eq!(
        app.job_error.as_deref(),
        Some(app.lang.text(Msg::ErrorArchiveViewUnavailable))
    );
    assert!(!app.jobs.is_busy());
    assert!(launcher.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_edit_that_got_past_the_refusal_is_still_read_only() {
    let dir = scratch("view-edit");
    let (mut app, launcher) = app_in_zip(dir.path(), Some(&dir.path().join("cache")));

    drop(app.update(Message::OpenExternal(fs::OpenKind::Edit)));

    assert_eq!(
        app.job_error.as_deref(),
        Some(app.lang.text(Msg::StatusArchiveReadOnly))
    );
    assert!(!app.jobs.is_busy());
    assert!(launcher.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_refused_archive_is_named_when_unpacking_fails() {
    let dir = scratch("view-refused");
    let archive = dir.path().join("x.7z");
    let mut app = app_with_copies(&dir.path().join("cache"));
    let slot = pretend_extracting(&mut app, &archive);

    drop(app.update(Message::ViewExtracted {
        slot,
        result: Err(RowFailure::Refused(fs::archive::SevenZipRefusal::Encrypted)),
        generation: app.ops.generation,
    }));

    assert_eq!(
        app.job_error.as_deref(),
        Some(app.lang.text(Msg::ErrorArchiveEncrypted))
    );
    assert!(app.ops.extracting.is_none());
    assert!(!app.jobs.is_busy());
}

#[tokio::test]
async fn a_result_of_a_stopped_run_removes_its_copy_and_starts_nothing() {
    let dir = scratch("view-stale");
    let archive = dir.path().join("x.zip");
    let mut app = app_with_copies(&dir.path().join("cache"));
    let launcher = Arc::new(Recorded::default());
    app = app.with_launcher(Arc::clone(&launcher) as Arc<dyn fs::Launcher>);
    let slot = app
        .ops
        .view_copies
        .as_mut()
        .unwrap()
        .slot(&archive)
        .unwrap();
    let copy = slot.join("a.txt");
    std::fs::write(&copy, "x").unwrap();
    let stale = app.ops.generation.wrapping_sub(1);

    drop(app.update(Message::ViewExtracted {
        slot: slot.clone(),
        result: Ok(copy),
        generation: stale,
    }));

    assert!(!slot.exists());
    assert!(launcher.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_copy_that_arrives_after_the_panel_left_is_removed_unseen() {
    let dir = scratch("view-late");
    let archive = dir.path().join("x.zip");
    let mut app = app_with_copies(&dir.path().join("cache"));
    let launcher = Arc::new(Recorded::default());
    app = app.with_launcher(Arc::clone(&launcher) as Arc<dyn fs::Launcher>);
    let slot = pretend_extracting(&mut app, &archive);
    let copy = slot.join("a.txt");
    std::fs::write(&copy, "x").unwrap();

    drop(app.update(Message::ViewExtracted {
        slot: slot.clone(),
        result: Ok(copy),
        generation: app.ops.generation,
    }));

    assert!(!slot.exists());
    assert!(launcher.0.lock().unwrap().is_empty());
    assert!(!app.jobs.is_busy());
}

/// Linux has no editor to fall back to, so an executable-looking copy has
/// no program to show it in unless the config names one.
#[cfg(not(any(target_os = "macos", windows)))]
#[tokio::test]
async fn a_script_without_an_editor_is_refused_without_naming_the_edit_key() {
    let dir = scratch("view-script");
    let script = dir.path().join("run.sh");
    std::fs::write(&script, "#!/bin/sh").unwrap();
    let launcher = Arc::new(Recorded::default());
    let mut app = App::with_environment(Environment::isolated())
        .0
        .with_launcher(Arc::clone(&launcher) as Arc<dyn fs::Launcher>);

    app.launch_external(fs::OpenKind::View, script, None, CopyOf::Archive);

    assert!(launcher.0.lock().unwrap().is_empty());
    let shown = app.job_error.unwrap();
    assert!(shown.contains("run.sh"), "{shown}");
    assert!(!shown.contains("F4"), "{shown}");
    assert_eq!(
        shown,
        fill_template(
            Language::default().text(Msg::ErrorArchiveViewExecutable),
            &[("{name}", "run.sh")]
        )
    );
}
