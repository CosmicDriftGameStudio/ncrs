//! Enter on a file opens it as the Finder does: by the system opener, after a
//! question when the file could run.

use std::ffi::OsString;
use std::sync::Mutex;

use crate::app::*;
use crate::fs::FileEntry;
use iced::keyboard::key::Named;

#[derive(Default)]
struct RecordingLauncher {
    calls: Mutex<Vec<fs::OpenCommand>>,
    fail_with: Option<&'static str>,
}

impl fs::Launcher for RecordingLauncher {
    fn launch(&self, command: &fs::OpenCommand) -> Result<(), String> {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push(command.clone());
        }
        self.fail_with
            .map_or(Ok(()), |reason| Err(reason.to_owned()))
    }
}

fn started(launcher: &RecordingLauncher) -> Vec<fs::OpenCommand> {
    launcher
        .calls
        .lock()
        .map(|calls| calls.clone())
        .unwrap_or_default()
}

fn file_at(name: &str) -> PathBuf {
    Path::new("/home/test/links").join(name)
}

fn app_on(name: &str, fail_with: Option<&'static str>) -> (App, Arc<RecordingLauncher>) {
    let launcher = Arc::new(RecordingLauncher {
        fail_with,
        ..RecordingLauncher::default()
    });
    let mut app =
        App::with_fixed_panels().with_launcher(Arc::clone(&launcher) as Arc<dyn fs::Launcher>);
    app.panels.left.entries = vec![FileEntry {
        name: OsString::from(name),
        path: file_at(name),
        is_dir: false,
        is_symlink: false,
        is_parent: false,
        size: 1,
        modified: None,
        folder_size: None,
        details: Default::default(),
    }];
    app.panels.left.selected = 0;
    (app, launcher)
}

fn press(app: &mut App, key: Named) {
    let routed = route_key(
        &app.prompt_key_state(),
        Key::Named(key),
        Modifiers::default(),
    )
    .expect("the key is routed");
    drop(app.update(routed));
}

#[test]
fn a_pdf_starts_the_system_opener_even_with_a_configured_viewer() {
    let (mut app, launcher) = app_on("report.pdf", None);
    app.open_programs.view = Some(config::ProgramLine::new("my-viewer", &[]));

    press(&mut app, Named::Enter);

    let expected = fs::system_open_command(file_at("report.pdf"));
    assert_eq!(started(&launcher), vec![expected]);
    assert!(app.ops.open_file_dialog.is_none());
}

#[test]
fn a_double_click_on_a_row_that_moved_since_the_first_click_opens_nothing() {
    let (mut app, launcher) = app_on("setup.command", None);

    drop(app.update(Message::Panel(PanelMessage::RowActivated {
        side: PanelSide::Left,
        index: 0,
        name: "notes.txt".into(),
    })));

    assert_eq!(app.ops.open_file_dialog, None);
    assert!(started(&launcher).is_empty());
}

#[test]
fn a_double_click_on_a_file_that_could_run_asks_first() {
    let (mut app, launcher) = app_on("setup.command", None);

    drop(app.update(Message::Panel(PanelMessage::RowActivated {
        side: PanelSide::Left,
        index: 0,
        name: "setup.command".into(),
    })));

    assert_eq!(app.ops.open_file_dialog, Some(file_at("setup.command")));
    assert!(started(&launcher).is_empty());
}

#[test]
fn the_context_menu_on_a_file_that_could_run_asks_first() {
    use crate::context_action_message::ContextActionMessage as _;
    let (mut app, launcher) = app_on("setup.command", None);

    drop(app.update(ncrs_files::context_menu::ContextAction::Open.message()));

    assert_eq!(app.ops.open_file_dialog, Some(file_at("setup.command")));
    assert!(started(&launcher).is_empty());
}

#[test]
fn a_file_that_could_run_asks_first_and_starts_nothing() {
    let (mut app, launcher) = app_on("setup.command", None);

    press(&mut app, Named::Enter);

    assert_eq!(app.ops.open_file_dialog, Some(file_at("setup.command")));
    assert_eq!(app.dialog_focus, ncrs_ops::open_file::CANCEL_FOCUS);
    assert!(started(&launcher).is_empty());
}

#[test]
fn enter_at_the_start_focus_cancels_and_a_held_enter_does_not_repeat() {
    let (mut app, launcher) = app_on("setup.command", None);
    press(&mut app, Named::Enter);

    assert!(!repeats(&Message::DialogActivate));
    assert!(!repeats(&Message::Panel(PanelMessage::OpenSelected)));
    press(&mut app, Named::Enter);

    assert!(app.ops.open_file_dialog.is_none());
    assert!(started(&launcher).is_empty());
}

#[test]
fn confirming_starts_the_file_exactly_once() {
    let (mut app, launcher) = app_on("setup.command", None);
    press(&mut app, Named::Enter);
    press(&mut app, Named::Tab);
    assert_eq!(app.dialog_focus, ncrs_ops::open_file::CONFIRM_FOCUS);

    press(&mut app, Named::Enter);
    drop(app.update(Message::OpenFileConfirm));

    assert_eq!(
        started(&launcher),
        vec![fs::system_open_command(file_at("setup.command"))]
    );
    assert!(app.ops.open_file_dialog.is_none());
}

#[test]
fn escape_closes_the_question_and_starts_nothing() {
    let (mut app, launcher) = app_on("setup.command", None);
    press(&mut app, Named::Enter);

    press(&mut app, Named::Escape);

    assert!(app.ops.open_file_dialog.is_none());
    assert!(started(&launcher).is_empty());
}

#[test]
fn a_running_queue_does_not_block_the_question_but_an_open_dialog_does() {
    let (mut app, _launcher) = app_on("setup.command", None);
    app.jobs.enqueue(jobs::Job {
        kind: jobs::JobKind::Extract,
        path: PathBuf::from("/x/a.zip"),
        total: None,
    });
    assert!(app.jobs.start_next(|_| Task::<Message>::none()).is_some());
    drop(app.update(Message::Panel(PanelMessage::OpenSelected)));
    assert!(app.ops.open_file_dialog.is_some());

    let (mut other, _other_launcher) = app_on("setup.command", None);
    other.open_delete_dialog_for_test(1, false);
    drop(other.update(Message::Panel(PanelMessage::OpenSelected)));
    assert!(other.ops.open_file_dialog.is_none());
}

#[test]
fn a_refusal_of_the_system_is_shown_in_the_status_bar() {
    let (mut app, _launcher) = app_on("report.pdf", Some("no opener"));

    press(&mut app, Named::Enter);

    let error = app.job_error_for_test().unwrap_or_default();
    assert!(error.contains("no opener"), "{error}");
    assert!(error.contains("report.pdf"), "{error}");
}

#[cfg(target_os = "macos")]
#[test]
fn an_app_bundle_asks_first_on_macos() {
    let (mut app, launcher) = app_on("Tool.app", None);
    app.panels.left.entries[0].is_dir = true;

    press(&mut app, Named::Enter);

    assert_eq!(app.ops.open_file_dialog, Some(file_at("Tool.app")));
    assert!(started(&launcher).is_empty());
}
