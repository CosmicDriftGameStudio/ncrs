use std::ffi::OsString;
use std::sync::Mutex;

use crate::app::*;
use crate::fs::FileEntry;

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

fn calls(launcher: &RecordingLauncher) -> Vec<fs::OpenCommand> {
    launcher
        .calls
        .lock()
        .map(|calls| calls.clone())
        .unwrap_or_default()
}

fn entry(name: &str, is_dir: bool) -> FileEntry {
    FileEntry {
        name: OsString::from(name),
        path: Path::new("/home/test/links").join(name),
        is_dir,
        is_symlink: false,
        is_parent: false,
        size: 1,
        modified: None,
        folder_size: None,
    }
}

fn app_with_terminal(
    line: config::ProgramLine,
    fail_with: Option<&'static str>,
) -> (App, Arc<RecordingLauncher>) {
    let launcher = Arc::new(RecordingLauncher {
        fail_with,
        ..RecordingLauncher::default()
    });
    let mut app =
        App::with_fixed_panels().with_launcher(Arc::clone(&launcher) as Arc<dyn fs::Launcher>);
    app.open_programs.terminal = Some(line);
    (app, launcher)
}

#[test]
fn the_terminal_starts_in_the_folder_under_the_cursor_with_the_path_filled_in() {
    let (mut app, launcher) = app_with_terminal(
        config::ProgramLine::new("myterm", &["--in", "{path}"]),
        None,
    );
    app.panels.left.entries = vec![entry("my photos", true)];
    drop(app.update(Message::RunCommand(KeyAction::OpenTerminal)));
    let launched = calls(&launcher);
    assert_eq!(launched.len(), 1);
    let folder = PathBuf::from("/home/test/links/my photos");
    assert_eq!(launched[0].program, OsString::from("myterm"));
    assert_eq!(
        launched[0].args,
        vec![OsString::from("--in"), folder.clone().into_os_string()]
    );
    assert_eq!(launched[0].current_dir, Some(folder));
}

#[test]
fn a_file_under_the_cursor_means_the_folder_of_the_panel_and_without_path_nothing_is_appended() {
    let (mut app, launcher) = app_with_terminal(config::ProgramLine::new("myterm", &["-x"]), None);
    app.panels.left.entries = vec![entry("note.txt", false)];
    drop(app.update(Message::RunCommand(KeyAction::OpenTerminal)));
    let launched = calls(&launcher);
    assert_eq!(launched.len(), 1);
    assert_eq!(launched[0].args, vec![OsString::from("-x")]);
    assert_eq!(
        launched[0].current_dir,
        Some(PathBuf::from("/home/test/links"))
    );
}

#[test]
fn a_terminal_that_does_not_start_shows_the_reason_in_the_status_bar() {
    let (mut app, _launcher) = app_with_terminal(
        config::ProgramLine::new("myterm", &[]),
        Some("no such file"),
    );
    drop(app.update(Message::RunCommand(KeyAction::OpenTerminal)));
    let error = app.job_error_for_test().unwrap_or_default();
    assert!(error.contains("no such file"), "{error}");
    assert!(error.contains("links"), "{error}");
}

#[test]
fn inside_an_archive_nothing_is_launched_and_the_status_bar_says_why() {
    let (mut app, launcher) = app_with_terminal(config::ProgramLine::new("myterm", &[]), None);
    app.panels.left.archive = Some(PathBuf::from("/x/fotos.zip"));
    drop(app.update(Message::RunCommand(KeyAction::OpenTerminal)));
    assert!(calls(&launcher).is_empty());
    assert!(app.job_error_for_test().is_some());
}
