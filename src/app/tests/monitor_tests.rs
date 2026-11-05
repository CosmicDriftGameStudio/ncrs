use ncrs_config::system_monitor::{StatusValue, SystemSettings};
use ncrs_monitor::MonitorMessage;

use super::settings_pages_tests::settle;
use crate::app::*;
use crate::config::from_text;

fn press(app: &App, key: Key) -> Option<Message> {
    route_key(&app.prompt_key_state(), key, Modifiers::empty())
}

fn toggle() -> Message {
    Message::Monitor(MonitorMessage::Toggle)
}

#[test]
fn the_key_opens_the_page_and_closes_it_again() {
    let mut app = App::with_fixed_panels();
    assert_eq!(press(&app, Key::Named(Named::F12)), Some(toggle()));
    drop(app.update(toggle()));
    assert!(app.monitor.is_open());
    assert_eq!(press(&app, Key::Named(Named::F12)), Some(toggle()));
    drop(app.update(toggle()));
    assert!(!app.monitor.is_open());
}

#[test]
fn escape_closes_the_page() {
    let mut app = App::with_fixed_panels();
    drop(app.update(toggle()));
    drop(app.update(Message::Monitor(MonitorMessage::Escape)));
    assert!(!app.monitor.is_open());
}

#[test]
fn pressing_the_badge_opens_the_page() {
    let mut app = App::with_fixed_panels();
    app.monitor
        .set_settings(SystemSettings::default().toggled(StatusValue::Cpu, true));
    let badge = app.monitor_badge();
    assert!(badge
        .as_ref()
        .is_some_and(|shown| shown.text.starts_with("CPU")));
    if let Some(shown) = badge {
        drop(app.update(shown.on_press));
    }
    assert!(app.monitor.is_open());
}

#[test]
fn there_is_no_badge_without_a_chosen_value() {
    assert!(App::with_fixed_panels().monitor_badge().is_none());
}

#[tokio::test]
async fn go_to_folder_leaves_an_archive_for_the_local_folder() {
    let folder = std::env::temp_dir();
    let mut app = App::with_fixed_panels();
    let side = app.panels.active;
    let archive = folder.join("v.zip");
    let panel = app.panel_mut(side);
    panel.path = archive.clone();
    panel.archive = Some(archive);
    settle(
        &mut app,
        Message::Monitor(MonitorMessage::OpenDrive(folder.clone())),
    )
    .await;
    assert_eq!(app.panel(side).path, folder);
    assert!(app.panel(side).archive.is_none());
}

#[test]
fn a_config_without_the_section_gives_the_defaults() {
    let system = from_text("[keys]\n").map(|config| config.system).ok();
    assert_eq!(system, Some(SystemSettings::default()));
}

#[test]
fn a_config_that_gives_copy_the_key_f12_keeps_working_and_f12_copies() {
    let loaded = from_text("[keys]\ncopy = \"F12\"\n");
    assert!(loaded.is_ok(), "{:?}", loaded.as_ref().err());
    let app = App::with_fixed_panels().with_config(loaded);
    assert_eq!(
        press(&app, Key::Named(Named::F12)),
        Some(KeyAction::Copy.message())
    );
}
