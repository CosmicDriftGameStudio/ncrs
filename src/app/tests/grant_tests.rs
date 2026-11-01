use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::app::*;

static PICKS: AtomicUsize = AtomicUsize::new(0);
static GRANTED: Mutex<Vec<(PathBuf, PathBuf)>> = Mutex::new(Vec::new());

fn counting_picker(_: &Path) -> Task<Option<PathBuf>> {
    PICKS.fetch_add(1, Ordering::SeqCst);
    Task::none()
}

/// A picker that must not be asked: the counter above is shared by the
/// tests that run beside each other, so "not asked" is not one of its
/// readings.
fn forbidden_picker(_: &Path) -> Task<Option<PathBuf>> {
    panic!("the folder dialog was opened")
}

fn recording_grant(dir: &Path, folder: &Path) -> Result<(), String> {
    let mut recorded = GRANTED.lock().map_err(|err| err.to_string())?;
    recorded.push((dir.to_path_buf(), folder.to_path_buf()));
    Ok(())
}

fn refusing_grant(_: &Path, _: &Path) -> Result<(), String> {
    Err("bookmark refused".to_owned())
}

fn sandboxed(readable: bool, grant: fn(&Path, &Path) -> Result<(), String>) -> App {
    let mut app = App::with_fixed_panels();
    app.pick_folder = counting_picker;
    app.sandbox = fs::SandboxAccess {
        active: true,
        grants_dir: Some(PathBuf::from("grants-of-this-test")),
        home_readable: if readable { || true } else { || false },
        grant,
        covers: |_, _| true,
    };
    app
}

fn send(app: &mut App, message: GrantMessage) {
    drop(app.update(Message::Grant(message)));
}

static STARTED_IN: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

fn recording_picker(start: &Path) -> Task<Option<PathBuf>> {
    STARTED_IN.lock().unwrap().push(start.to_path_buf());
    Task::none()
}

#[test]
fn a_blocked_folder_opens_the_dialog_at_the_folder() {
    let mut app = sandboxed(true, recording_grant);
    app.pick_folder = recording_picker;
    let blocked = std::env::temp_dir().join("ncrs-blocked-test-folder");
    send(&mut app, GrantMessage::OpenFor(blocked));
    assert!(STARTED_IN.lock().unwrap().contains(&std::env::temp_dir()));
}

#[cfg(target_os = "macos")]
#[test]
fn the_blocked_error_asks_for_a_grant_only_in_the_sandbox() {
    let blocked = Path::new("/Users");
    assert_eq!(
        blocked_access_press(true, blocked),
        Some(Message::Grant(GrantMessage::OpenFor(blocked.to_path_buf())))
    );
    assert_eq!(
        blocked_access_press(false, blocked),
        Some(Message::OpenPrivacySettings)
    );
}

/// What the tasks `message` starts report, not yet delivered.
async fn reports(app: &mut App, message: GrantMessage) -> Vec<Message> {
    use iced_test::futures::futures::StreamExt as _;
    use iced_test::runtime::{task, Action};
    let mut found = Vec::new();
    if let Some(mut stream) = task::into_stream(app.update(Message::Grant(message))) {
        while let Some(action) = stream.next().await {
            if let Action::Output(reported) = action {
                found.push(reported);
            }
        }
    }
    found
}

/// Picks `folder` and delivers the answer of the thread that saves it.
async fn pick(app: &mut App, folder: PathBuf) {
    for reported in reports(app, GrantMessage::Picked(Some(folder))).await {
        drop(app.update(reported));
    }
}

fn granted_for(folder: &Path) -> Vec<PathBuf> {
    GRANTED
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, granted)| granted == folder)
        .map(|(dir, _)| dir.clone())
        .collect()
}

#[test]
fn the_dialog_opens_only_in_the_sandbox_while_home_cannot_be_read() {
    let mut closed_home = sandboxed(false, recording_grant);
    closed_home.open_grant_dialog_if_needed();
    assert!(closed_home.grant_dialog);
    assert!(closed_home.blocking_overlay_is_open());

    let mut open_home = sandboxed(true, recording_grant);
    open_home.open_grant_dialog_if_needed();
    assert!(!open_home.grant_dialog);

    let mut outside = sandboxed(false, recording_grant);
    outside.sandbox.active = false;
    outside.open_grant_dialog_if_needed();
    assert!(!outside.grant_dialog);
}

#[test]
fn a_normal_start_opens_no_dialog() {
    let (app, _) = App::with_environment(Environment::isolated());
    assert!(!app.grant_dialog);
    assert!(!app.sandbox.active);
}

#[test]
fn later_closes_the_dialog_and_grants_nothing() {
    let mut app = sandboxed(false, recording_grant);
    app.open_grant_dialog_if_needed();

    send(&mut app, GrantMessage::Later);

    assert!(!app.grant_dialog);
    assert!(!app.blocking_overlay_is_open());
    assert!(
        app.sandbox.active,
        "the sandbox stays on, only the dialog goes"
    );
    assert!(app.status_error().is_none());
}

#[test]
fn escape_is_later_and_enter_presses_the_focused_button() {
    let mut app = sandboxed(false, recording_grant);
    app.open_grant_dialog_if_needed();
    let route = |current: &App, key: Named| {
        route_key(
            &current.prompt_key_state(),
            Key::Named(key),
            Modifiers::default(),
        )
    };

    assert_eq!(
        route(&app, Named::Escape),
        Some(Message::Grant(GrantMessage::Later))
    );
    assert_eq!(app.dialog_focus, grant_ui::GRANT_FOCUS);
    assert_eq!(route(&app, Named::Enter), Some(Message::DialogActivate));

    drop(app.update(Message::DialogFocus(-1)));
    assert_eq!(app.dialog_focus, grant_ui::LATER_FOCUS);
    drop(app.update(Message::DialogActivate));
    assert!(!app.grant_dialog, "Enter on Later closes it");
}

#[test]
fn choosing_opens_the_folder_dialog_and_keeps_the_dialog_up() {
    let mut app = sandboxed(false, recording_grant);
    app.open_grant_dialog_if_needed();
    let picks = PICKS.load(Ordering::SeqCst);

    drop(app.update(Message::DialogActivate));

    assert!(PICKS.load(Ordering::SeqCst) > picks);
    assert!(app.grant_dialog);
}

#[test]
fn a_cancelled_folder_dialog_changes_nothing() {
    let mut app = sandboxed(false, recording_grant);
    app.open_grant_dialog_if_needed();

    send(&mut app, GrantMessage::Picked(None));

    assert!(app.grant_dialog);
    assert!(app.status_error().is_none());
}

#[tokio::test]
async fn a_chosen_folder_is_saved_off_the_ui_thread_and_both_panels_load_again() {
    let folder = PathBuf::from("chosen-and-saved");
    let mut app = sandboxed(false, recording_grant);
    app.open_grant_dialog_if_needed();
    let before = (app.panels.left.request_id, app.panels.right.request_id);

    let reported = reports(&mut app, GrantMessage::Picked(Some(folder.clone()))).await;

    assert!(!app.grant_dialog, "the dialog goes at the choice");
    assert_eq!(
        (app.panels.left.request_id, app.panels.right.request_id),
        before,
        "nothing reloads before the grant is saved"
    );
    assert_eq!(
        reported,
        [Message::Grant(GrantMessage::Saved {
            folder: folder.clone(),
            result: Ok(()),
        })]
    );
    assert_eq!(
        granted_for(&folder),
        vec![PathBuf::from("grants-of-this-test")]
    );
    for message in reported {
        drop(app.update(message));
    }
    assert_ne!(app.panels.left.request_id, before.0);
    assert_ne!(app.panels.right.request_id, before.1);
    assert!(app.notice.as_deref().unwrap().contains("chosen-and-saved"));
    assert!(app.status_error().is_none());
}

#[tokio::test]
async fn a_grant_that_cannot_be_saved_is_reported_and_loads_nothing() {
    let mut app = sandboxed(false, refusing_grant);
    app.open_grant_dialog_if_needed();
    let before = (app.panels.left.request_id, app.panels.right.request_id);

    pick(&mut app, PathBuf::from("refused")).await;

    let error = app.status_error().unwrap();
    assert!(
        error.contains("refused") && error.contains("bookmark refused"),
        "{error}"
    );
    assert!(!app.grant_dialog, "the status bar must not stay dimmed");
    assert_eq!(
        (app.panels.left.request_id, app.panels.right.request_id),
        before
    );
}

#[tokio::test]
async fn without_a_data_folder_the_grant_is_refused_and_not_attempted() {
    let folder = PathBuf::from("no-data-folder");
    let mut app = sandboxed(false, recording_grant);
    app.sandbox.grants_dir = None;

    pick(&mut app, folder.clone()).await;

    assert!(granted_for(&folder).is_empty());
    assert!(app.status_error().unwrap().contains("data folder"));
}

#[test]
fn the_command_opens_the_folder_dialog_in_the_sandbox() {
    let mut app = sandboxed(true, recording_grant);
    let picks = PICKS.load(Ordering::SeqCst);

    drop(app.update(KeyAction::GrantFolderAccess.message()));

    assert!(PICKS.load(Ordering::SeqCst) > picks);
    assert!(!app.grant_dialog, "only the first start shows the dialog");
}

#[test]
fn the_command_does_nothing_while_another_dialog_is_up() {
    let mut app = sandboxed(true, recording_grant);
    app.pick_folder = forbidden_picker;
    app.open_delete_dialog_for_test(1, false);

    drop(app.update(KeyAction::GrantFolderAccess.message()));
}

#[test]
fn outside_the_sandbox_the_command_says_it_is_not_needed() {
    let mut app = App::with_fixed_panels();
    app.pick_folder = forbidden_picker;

    drop(app.update(KeyAction::GrantFolderAccess.message()));

    assert!(app.notice.as_deref().unwrap().contains("App Store"));
}

#[test]
fn the_command_is_listed_only_in_the_sandbox() {
    let listed =
        crate::commands::all().any(|command| command.action == KeyAction::GrantFolderAccess);
    assert_eq!(listed, fs::sandbox_active());
    assert!(!fs::sandbox_active(), "tests never run in the sandbox");
}

#[test]
fn the_app_store_build_keeps_its_key_apart_from_the_installed_one() {
    assert_eq!(APP_STORE_KEY_SERVICE, "ncrs-appstore");
}

#[test]
fn the_command_has_no_key_and_works_in_an_archive() {
    assert!(KeyAction::GrantFolderAccess.allowed_in_archive());
    assert!(Keymap::built_in()
        .bindings_of(KeyAction::GrantFolderAccess)
        .next()
        .is_none());
}
