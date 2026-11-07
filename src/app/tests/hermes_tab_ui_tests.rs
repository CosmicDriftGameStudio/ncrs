//! The title bar with all three page tabs: Settings, System and Hermes.

use super::hermes_chat_ui_tests::{matches_snapshot, opened};
use crate::app::*;
use crate::i18n::Language;

#[test]
fn the_title_bar_shows_the_hermes_tab_after_settings_and_system() {
    for (lang, name) in [
        (Language::German, "title_bar_hermes_tab_de"),
        (Language::English, "title_bar_hermes_tab_en"),
    ] {
        let (mut app, _) = opened(lang);
        drop(app.update(Message::Settings(SettingsMessage::Toggle)));
        drop(app.update(Message::Monitor(MonitorMessage::Toggle)));
        drop(app.update(Message::ShowTab(ShownTab::Hermes)));
        matches_snapshot(&app, name);
    }
}
