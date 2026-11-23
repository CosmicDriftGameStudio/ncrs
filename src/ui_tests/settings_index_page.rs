use super::app_simulator as simulator;
use std::path::PathBuf;

use super::*;
use crate::config::{AiSetting, Filing, FilingLocation};
use crate::messages::{HermesOverviewMessage, SettingsMessage};
use ncrs_intake::LocationMode;

fn configured() -> Filing {
    let at = |path: &str, mode, is_target| FilingLocation {
        path: PathBuf::from(path),
        mode,
        images: false,
        is_target,
        transfer: Default::default(),
        continuous: false,
    };
    Filing {
        enabled: true,
        locations: vec![
            at("/home/test/Scans", LocationMode::Inbox, false),
            at("/home/test/Documents", LocationMode::Archive, true),
        ],
        load: Default::default(),
        ai: AiSetting::Local { threads: None },
        default_pattern: None,
        own_names: Vec::new(),
        own_ibans: Vec::new(),
        own_tax_ids: Vec::new(),
        first_run: false,
    }
}

fn clicks(app: &App, label: &str) -> Vec<Message> {
    let mut sim = simulator(app);
    sim.click(label).unwrap();
    sim.into_messages().collect()
}

fn tall_simulator(app: &App) -> Simulator<'_, Message, iced::Theme> {
    Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1200.0, 1100.0),
        app.view(),
    )
}

fn overview_message(message: HermesOverviewMessage) -> Message {
    Message::HermesOverview(message)
}

#[test]
fn the_settings_hold_the_protection_and_the_question_before_starting_over() {
    let missing =
        App::with_index_settings(Some(Phase::Ready), configured()).with_missing_key_for_test();
    let mut sim = tall_simulator(&missing);
    sim.click("Start over …").unwrap();
    assert_eq!(
        sim.into_messages().collect::<Vec<_>>(),
        [Message::Settings(SettingsMessage::FocusThen(
            6,
            Some(Box::new(SettingsMessage::Protection(2)))
        ))]
    );
    let asking = App::with_index_settings(Some(Phase::Ready), configured())
        .with_start_over_question_for_test();
    assert_eq!(
        clicks(&asking, "Cancel"),
        [overview_message(HermesOverviewMessage::CancelStartOver)]
    );
    assert_eq!(
        clicks(&asking, "Set the index aside"),
        [overview_message(HermesOverviewMessage::ConfirmStartOver)]
    );
}

fn settings_over_index(encrypted: bool) -> App {
    App::with_index_settings(Some(Phase::Ready), configured()).with_index_for_test(encrypted)
}

fn both_languages_tall(app: impl Fn() -> App, name: &str) {
    for (lang, suffix) in [(Language::English, "en"), (Language::German, "de")] {
        let matches = tall_simulator(&app().in_language(lang))
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_reference_image(format!("tests/snapshots/{name}_{suffix}.png"))
            .expect("snapshot comparison");
        assert!(matches, "the settings do not match {name}_{suffix}.png");
    }
}

#[test]
fn the_settings_with_the_protection_match_their_snapshots() {
    both_languages_tall(|| settings_over_index(true), "settings_hermes_protection");
    both_languages_tall(
        || settings_over_index(false),
        "settings_hermes_protection_plain",
    );
    both_languages_tall(
        || settings_over_index(true).with_missing_key_for_test(),
        "settings_hermes_key_missing",
    );
    both_languages_tall(
        || settings_over_index(true).with_start_over_question_for_test(),
        "settings_hermes_reset_confirm",
    );
}
