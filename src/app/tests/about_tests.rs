use std::sync::Mutex;

use crate::app::*;

#[derive(Default)]
struct RecordingLauncher(Mutex<Vec<fs::OpenCommand>>);

impl fs::Launcher for RecordingLauncher {
    fn launch(&self, command: &fs::OpenCommand) -> Result<(), String> {
        if let Ok(mut calls) = self.0.lock() {
            calls.push(command.clone());
        }
        Ok(())
    }
}

fn calls(launcher: &RecordingLauncher) -> Vec<fs::OpenCommand> {
    launcher
        .0
        .lock()
        .map(|calls| calls.clone())
        .unwrap_or_default()
}

fn send(app: &mut App, message: AboutMessage) {
    drop(app.update(Message::About(message)));
}

fn app_with_launcher(finder: fn() -> Option<PathBuf>) -> (App, Arc<RecordingLauncher>) {
    let launcher = Arc::new(RecordingLauncher::default());
    let mut app = App::with_fixed_panels()
        .with_licenses_finder(finder)
        .with_launcher(Arc::clone(&launcher) as Arc<dyn fs::Launcher>);
    app.app_version = "1.2.3";
    (app, launcher)
}

#[test]
fn the_about_action_opens_the_card_and_close_and_escape_close_it() {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::RunCommand(KeyAction::About)));
    assert!(app.about_for_test().is_some());
    assert_eq!(app.dialog_focus, about_ui::CLOSE_FOCUS);
    send(&mut app, AboutMessage::Close);
    assert!(app.about_for_test().is_none());

    drop(app.update(Message::RunCommand(KeyAction::About)));
    let prompt = PromptKeyState {
        about: true,
        ..PromptKeyState::default()
    };
    assert_eq!(
        card_close_message(&prompt),
        Some(Message::About(AboutMessage::Close))
    );
}

#[test]
fn enter_on_the_close_button_closes_the_card() {
    let mut app = App::with_about_card(AboutCard::new());
    drop(app.update(Message::DialogActivate));
    assert!(app.about_for_test().is_none());
}

#[test]
fn a_found_licenses_file_is_launched_once_with_its_path_as_last_argument() {
    let (mut app, launcher) =
        app_with_launcher(|| Some(PathBuf::from("/opt/ncrs/THIRD_PARTY_LICENSES.html")));
    send(&mut app, AboutMessage::Open);
    send(&mut app, AboutMessage::OpenLicenses);
    let launched = calls(&launcher);
    assert_eq!(launched.len(), 1);
    assert_eq!(
        launched.first().and_then(|call| call.args.last()),
        Some(&std::ffi::OsString::from(
            "/opt/ncrs/THIRD_PARTY_LICENSES.html"
        ))
    );
    assert!(app.about_for_test().is_some());
}

#[test]
fn a_missing_licenses_file_launches_nothing_and_points_to_the_release() {
    let (mut app, launcher) = app_with_launcher(|| None);
    send(&mut app, AboutMessage::Open);
    send(&mut app, AboutMessage::OpenLicenses);
    assert!(calls(&launcher).is_empty());
    assert!(app
        .about_for_test()
        .is_some_and(AboutCard::licenses_missing));
}

#[test]
fn the_repository_button_launches_the_address() {
    let (mut app, launcher) = app_with_launcher(|| None);
    send(&mut app, AboutMessage::Open);
    send(&mut app, AboutMessage::OpenRepository);
    let launched = calls(&launcher);
    assert_eq!(launched.len(), 1);
    assert_eq!(
        launched.first().and_then(|call| call.args.last()),
        Some(&std::ffi::OsString::from(REPOSITORY_URL))
    );
}
