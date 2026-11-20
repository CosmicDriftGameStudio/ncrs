use crate::e2e_tests::*;
use iced::keyboard::key::Named;

/// Explicit programs, so the asserts do not depend on the OS defaults.
fn test_programs() -> crate::config::Config {
    let mut config = crate::config::Config::default();
    config.open.view = Some(crate::config::ProgramLine::new("viewer", &["--x"]));
    config.open.edit = Some(crate::config::ProgramLine::new("editor", &[]));
    config
}

fn recording_session(
    dir: &Path,
    load: impl Fn() -> Result<crate::config::Config, crate::config::ConfigError> + 'static,
) -> (
    std::sync::Arc<RecordingLauncher>,
    Session<impl iced::Program<State = App, Message = Message> + 'static>,
) {
    let launcher = std::sync::Arc::new(RecordingLauncher::default());
    let recorder = std::sync::Arc::clone(&launcher);
    let shared: std::sync::Arc<dyn crate::fs::Launcher> = recorder;
    let session = session_with_config(dir, load, shared);
    (launcher, session)
}

fn launcher_session(
    dir: &Path,
) -> (
    std::sync::Arc<RecordingLauncher>,
    Session<impl iced::Program<State = App, Message = Message> + 'static>,
) {
    recording_session(dir, || Ok(test_programs()))
}

fn command(program: &str, args: &[&str], last: &Path) -> crate::fs::OpenCommand {
    let mut all: Vec<std::ffi::OsString> = args.iter().map(std::ffi::OsString::from).collect();
    all.push(last.as_os_str().to_owned());
    crate::fs::OpenCommand {
        program: program.into(),
        args: all,
        ..crate::fs::OpenCommand::default()
    }
}

#[test]
fn f3_and_f4_hand_the_file_under_the_cursor_to_the_launcher() {
    let dir = scratch("open-file");
    write(&dir.path().join("alpha.txt"), "a");
    write(&dir.path().join("beta.txt"), "b");
    let (launcher, mut session) = launcher_session(dir.path());
    cursor_to(&mut session, "beta.txt");
    let beta = std::fs::canonicalize(dir.path().join("beta.txt")).expect("beta");

    session.press(Key::Named(Named::F3));
    session.press(Key::Named(Named::F4));

    assert_eq!(
        launcher.calls(),
        vec![
            command("viewer", &["--x"], &beta),
            command("editor", &[], &beta)
        ]
    );
}

/// As in classic dual-panel file managers: tagged rows do not turn F3 into "view all of them".
#[test]
fn tags_do_not_change_what_f3_opens() {
    let dir = scratch("open-tagged");
    write(&dir.path().join("alpha.txt"), "a");
    write(&dir.path().join("beta.txt"), "b");
    let (launcher, mut session) = launcher_session(dir.path());
    cursor_to(&mut session, "alpha.txt");
    session.press(Key::Named(Named::Insert));
    cursor_to(&mut session, "beta.txt");

    session.press(Key::Named(Named::F3));

    let calls = launcher.calls();
    assert_eq!(calls.len(), 1);
    assert!(
        calls[0]
            .args
            .last()
            .is_some_and(|last| last.to_string_lossy().ends_with("beta.txt")),
        "opened {:?}",
        calls[0].args
    );
}

#[test]
fn f3_and_f4_on_a_directory_or_the_parent_row_start_nothing() {
    let dir = scratch("open-dir");
    let (launcher, mut session) = launcher_session(dir.path());
    assert_eq!(
        selected(&session.app, PanelSide::Left).as_deref(),
        Some("..")
    );
    session.press(Key::Named(Named::F3));
    session.press(Key::Named(Named::F4));

    cursor_to(&mut session, "files");
    session.press(Key::Named(Named::F3));
    session.press(Key::Named(Named::F4));

    assert!(launcher.calls().is_empty(), "{:?}", launcher.calls());
}

#[cfg(unix)]
#[test]
fn f3_on_a_symlink_passes_the_link_path() {
    let dir = scratch("open-link");
    write(&dir.path().join("target.txt"), "t");
    let root = std::fs::canonicalize(dir.path()).expect("root");
    std::os::unix::fs::symlink(root.join("target.txt"), root.join("link.txt")).expect("link");
    let (launcher, mut session) = launcher_session(dir.path());
    cursor_to(&mut session, "link.txt");

    session.press(Key::Named(Named::F3));

    assert_eq!(
        launcher.calls(),
        vec![command("viewer", &["--x"], &root.join("link.txt"))]
    );
}

#[cfg(unix)]
fn executable_script(label: &str) -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = scratch(label);
    let script = dir.path().join("tool");
    write(&script, "#!/bin/sh\n");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    dir
}

/// The system opener would run a script. With an edit program configured
/// the file goes there, on every platform, and never to the viewer.
#[cfg(unix)]
#[test]
fn f3_on_an_executable_goes_to_the_configured_editor() {
    let dir = executable_script("open-exec");
    let (launcher, mut session) = launcher_session(dir.path());
    cursor_to(&mut session, "tool");

    session.press(Key::Named(Named::F3));

    let tool = std::fs::canonicalize(dir.path().join("tool")).expect("tool");
    assert_eq!(launcher.calls(), vec![command("editor", &[], &tool)]);
}

/// With no edit program the default decides: a text editor where the OS
/// has one, a refusal naming the file elsewhere.
#[cfg(unix)]
#[test]
fn f3_on_an_executable_with_the_default_config_follows_the_os() {
    let dir = executable_script("open-exec-default");
    let (launcher, mut session) = recording_session(dir.path(), || Ok(Default::default()));
    cursor_to(&mut session, "tool");

    session.press(Key::Named(Named::F3));

    let calls = launcher.calls();
    if cfg!(target_os = "macos") {
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].program, "open");
        assert_eq!(
            calls[0]
                .args
                .first()
                .map(|a| a.to_string_lossy().into_owned())
                .as_deref(),
            Some("-t")
        );
    } else {
        assert!(calls.is_empty(), "started {calls:?}");
        let message = session.app.job_error_for_test().expect("no refusal shown");
        assert!(message.contains("tool"), "{message}");
    }
}

/// The whole chain: a real file on disk, read by `config::load`, applied
/// to the app, and what F3 starts is what the file said.
#[test]
fn f3_runs_the_program_a_config_file_names() {
    let dir = scratch("open-config-file");
    write(&dir.path().join("alpha.txt"), "a");
    let config_path = dir.path().join("config.toml");
    write(
        &config_path,
        "version = 1\n\n[open]\nview = [\"viewer\", \"--x\"]\n",
    );
    let (launcher, mut session) =
        recording_session(dir.path(), move || crate::config::load(&config_path));
    cursor_to(&mut session, "alpha.txt");

    session.press(Key::Named(Named::F3));

    let alpha = std::fs::canonicalize(dir.path().join("alpha.txt")).expect("alpha");
    assert_eq!(launcher.calls(), vec![command("viewer", &["--x"], &alpha)]);
    assert_eq!(session.app.job_error_for_test(), None);
}

/// A broken file: the line shows in the status bar, and the defaults
/// apply, so F3 starts the platform's own viewer.
#[test]
fn a_broken_config_file_reports_its_line_and_the_defaults_apply() {
    let dir = scratch("open-config-broken");
    write(&dir.path().join("alpha.txt"), "a");
    let config_path = dir.path().join("config.toml");
    write(&config_path, "version = 1\n[open]\ncolour = \"red\"\n");
    let shown_path = config_path.display().to_string();
    let (launcher, mut session) =
        recording_session(dir.path(), move || crate::config::load(&config_path));

    let message = session
        .app
        .config_error_for_test()
        .expect("no error shown")
        .to_owned();
    assert!(message.contains(&shown_path), "{message}");
    assert!(message.contains("line 3"), "{message}");
    assert_eq!(session.app.status_error(), Some(message.as_str()));

    // Cursor, panel switch and type-ahead do not take it away.
    session.press(Key::Named(Named::ArrowDown));
    session.press(Key::Named(Named::Tab));
    session.press(Key::Character("a".into()));
    assert_eq!(session.app.status_error(), Some(message.as_str()));

    // Escape closes the menu first and leaves the message, the next one
    // dismisses it.
    session.press_with(Key::Named(Named::F10), Modifiers::SHIFT);
    assert!(session.app.context_menu_for_test().is_some());
    session.press(Key::Named(Named::Escape));
    assert!(session.app.context_menu_for_test().is_none());
    assert_eq!(session.app.status_error(), Some(message.as_str()));

    // So does a dialog: Escape answers it and the message stays.
    session.press(Key::Named(Named::Tab));
    cursor_to(&mut session, "alpha.txt");
    session.press(Key::Named(Named::F8));
    assert!(session.app.delete_dialog_is_open(), "F8 opened no dialog");
    session.press(Key::Named(Named::Escape));
    assert!(!session.app.delete_dialog_is_open());
    assert!(dir.path().join("alpha.txt").exists());
    assert_eq!(session.app.status_error(), Some(message.as_str()));

    session.press(Key::Named(Named::Escape));
    assert_eq!(session.app.status_error(), None);

    session.press(Key::Named(Named::F3));

    let calls = launcher.calls();
    assert_eq!(calls.len(), 1);
    let default_viewer = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    assert_eq!(calls[0].program, default_viewer);
}
