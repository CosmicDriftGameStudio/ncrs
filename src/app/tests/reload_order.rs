use crate::app::*;

/// Removes itself, so a failed assertion does not leave a directory that
/// makes the next run fail.
fn scratch(label: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("ncrs-order-{label}-"))
        .tempdir()
        .expect("a scratch directory")
}

/// Regression: F7 started the create and the reload at the same time with
/// `Task::batch`. A read that finished first saw a directory without the new
/// entry, and the selection landed on whatever was there instead — the new
/// directory silently missing from the panel.
///
/// The fix puts the reload behind the create's result. This checks the
/// ordering without timing: while the create is still in flight, no reload
/// may have been issued.
#[test]
fn the_reload_waits_for_the_create() {
    let scratch = scratch("waits");
    let base = scratch.path().to_path_buf();
    std::fs::create_dir_all(&base).unwrap();

    let mut app = App::with_environment(Environment::isolated()).0;
    app.panels.left.path = base.clone();
    app.ops.prompt = Some(Prompt::create_dir(base.clone()));
    app.ops.prompt.as_mut().unwrap().set_name("neu".into());
    // Submitting queues the reload instead of running it.
    let _ = app.update(Message::PromptSubmit);
    assert!(
        app.panels.pending_reload.is_some(),
        "no reload was queued, so the panel would never refresh"
    );
    assert_ne!(
        app.ops.prompt_request_id, 0,
        "the request id was not advanced, so a stale result could land"
    );

    // Still busy: the create has not reported.
    assert!(app.ops.prompt.as_ref().unwrap().busy());
}

/// The queued reload runs when the create reports, and clears itself so a
/// later result cannot trigger a second read.
#[test]
fn the_reload_runs_when_the_create_reports() {
    let scratch = scratch("runs");
    let base = scratch.path().to_path_buf();
    std::fs::create_dir_all(&base).unwrap();

    let mut app = App::with_environment(Environment::isolated()).0;
    app.panels.left.path = base.clone();
    app.ops.prompt = Some(Prompt::create_dir(base.clone()));
    app.ops.prompt.as_mut().unwrap().set_name("spaet".into());
    app.ops.prompt_request_id = 1;
    app.panels.pending_reload = Some(PendingReload {
        side: PanelSide::Left,
        name: "spaet".into(),
    });

    let _ = app.update(Message::PromptFinished {
        prompt: PromptKind::CreateDir,
        request_id: 1,
        result: Ok(()),
    });

    assert!(
        app.panels.pending_reload.is_none(),
        "the reload was not consumed and would run again"
    );
    assert!(
        app.ops.prompt.is_none(),
        "the prompt stayed open after success"
    );
}

/// On failure the prompt stays open with the error, and the reload still
/// runs: the name may belong to something that appeared in the meantime.
#[test]
fn a_failure_still_reloads() {
    let scratch = scratch("failure");
    let base = scratch.path().to_path_buf();
    std::fs::create_dir_all(&base).unwrap();

    let mut app = App::with_environment(Environment::isolated()).0;
    app.panels.left.path = base.clone();
    app.ops.prompt = Some(Prompt::create_dir(base.clone()));
    app.ops.prompt.as_mut().unwrap().set_name("weg".into());
    app.ops.prompt_request_id = 2;
    app.panels.pending_reload = Some(PendingReload {
        side: PanelSide::Left,
        name: "weg".into(),
    });

    let _ = app.update(Message::PromptFinished {
        prompt: PromptKind::CreateDir,
        request_id: 2,
        result: Err(fs::CreateDirError::AlreadyExists {
            path: base.join("weg"),
            source: std::io::Error::new(std::io::ErrorKind::AlreadyExists, "exists"),
        }),
    });

    assert!(
        app.panels.pending_reload.is_none(),
        "a failed create left a reload queued"
    );
    assert!(
        app.ops.prompt.is_some(),
        "the prompt closed on failure, so the user cannot correct the name"
    );
    assert!(!app.ops.prompt.as_ref().unwrap().busy());
}

#[derive(Default)]
struct RecordingLauncher(std::sync::Mutex<Vec<fs::OpenCommand>>);

impl fs::Launcher for RecordingLauncher {
    fn launch(&self, command: &fs::OpenCommand) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|poisoned| poisoned.to_string())?
            .push(command.clone());
        Ok(())
    }
}

/// An app on `base` with the new-file prompt submitted for `name`, and the
/// result of the real `create_file` delivered, as the runtime would.
async fn submit_new_file(base: &Path, name: &str) -> (App, Arc<RecordingLauncher>) {
    let launcher = Arc::new(RecordingLauncher::default());
    let mut app = App::with_environment(Environment::isolated())
        .0
        .with_launcher(Arc::clone(&launcher) as Arc<dyn fs::Launcher>);
    app.open_programs = config::OpenPrograms {
        edit: Some(config::ProgramLine::new("editor", &[])),
        ..config::OpenPrograms::default()
    };
    app.panels.left.path = base.to_path_buf();
    drop(app.update(Message::CreateFilePrompt));
    app.ops.prompt.as_mut().unwrap().set_name(name.into());
    drop(app.update(Message::PromptSubmit));

    let result = fs::create_file(base.join(name)).await;
    drop(app.update(Message::PromptFinished {
        prompt: PromptKind::CreateFile,
        request_id: app.ops.prompt_request_id,
        result,
    }));
    (app, launcher)
}

#[tokio::test]
async fn a_new_file_is_created_opened_in_the_editor_and_selected() {
    let scratch = scratch("new-file");
    let base = scratch.path().to_path_buf();

    let (mut app, launcher) = submit_new_file(&base, "note.txt").await;

    assert!(
        app.ops.prompt.is_none(),
        "the prompt stayed open after success"
    );
    assert_eq!(std::fs::read(base.join("note.txt")).unwrap(), b"");
    let calls = launcher.0.lock().unwrap().clone();
    assert_eq!(calls.len(), 1, "the editor was not started exactly once");
    assert_eq!(calls[0].program, "editor");
    assert_eq!(
        calls[0].args.last().unwrap(),
        base.join("note.txt").as_os_str()
    );

    // The reload that follows puts the cursor on the new name.
    let listing = fs::read_directory(base.clone(), fs::SortKey::default()).await;
    let request_id = app.panels.left.begin_load();
    drop(app.update(Message::Panel(PanelMessage::DirectoryLoaded {
        side: PanelSide::Left,
        request_id,
        path: listing.path,
        entries: listing.entries,
        error: listing.error,
        sorted_by: listing.sorted_by,
        select: Some("note.txt".into()),
        archive: listing.archive,
    })));
    assert_eq!(
        app.panels
            .left
            .selected_entry()
            .map(|entry| entry.path.clone()),
        Some(base.join("note.txt"))
    );
}

#[tokio::test]
async fn an_existing_name_is_refused_and_nothing_is_opened() {
    let scratch = scratch("new-file-taken");
    let base = scratch.path().to_path_buf();
    std::fs::write(base.join("note.txt"), b"keep me").unwrap();

    let (app, launcher) = submit_new_file(&base, "note.txt").await;

    let prompt = app
        .ops
        .prompt
        .as_ref()
        .expect("the prompt stays for a retry");
    assert!(!prompt.busy());
    assert_eq!(prompt.name(), "note.txt");
    assert!(prompt.error().is_some_and(|text| text.contains("note.txt")));
    assert_eq!(std::fs::read(base.join("note.txt")).unwrap(), b"keep me");
    assert!(launcher.0.lock().unwrap().is_empty());
}

/// A result for a request the app has moved past changes nothing — not the
/// prompt, and not a queued reload either.
#[test]
fn a_stale_result_touches_neither() {
    let mut app = App::with_environment(Environment::isolated()).0;
    app.ops.prompt_request_id = 5;
    app.panels.pending_reload = Some(PendingReload {
        side: PanelSide::Left,
        name: "x".into(),
    });

    let _ = app.update(Message::PromptFinished {
        prompt: PromptKind::CreateDir,
        request_id: 4,
        result: Ok(()),
    });

    assert!(app.ops.prompt.is_none());
    assert!(
        app.panels.pending_reload.is_some(),
        "a stale result consumed the queued reload"
    );
}
