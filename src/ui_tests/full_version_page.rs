//! Snapshots of the Full version page of the settings tab, one per state.

use iced_test::Simulator;
use ncrs_store::{Entitlement, FullKind};
use ncrs_ui_kit::snapshot_support::ReferenceSnapshot as _;

use super::app_simulator as simulator;
use crate::app::App;
use crate::i18n::Language;
use crate::messages::SettingsFeature;

fn page(entitlement: Entitlement, lang: Language) -> App {
    App::with_settings_file(
        SettingsFeature::FullVersion,
        Ok(crate::config::from_text("[theme]\npreset = \"cosmic\"\n").unwrap()),
    )
    .with_entitlement(entitlement)
    .in_language(lang)
}

fn matches_snapshot(
    simulator: &mut Simulator<'_, crate::messages::Message, iced::Theme>,
    name: &str,
) {
    let matches = simulator
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image(format!("tests/snapshots/{name}.png"))
        .expect("snapshot comparison");
    assert!(
        matches,
        "the page does not match tests/snapshots/{name}.png"
    );
}

fn both_languages(entitlement: Entitlement, name: &str) {
    for (lang, suffix) in [(Language::German, "de"), (Language::English, "en")] {
        let app = page(entitlement, lang);
        matches_snapshot(&mut simulator(&app), &format!("{name}_{suffix}"));
    }
}

#[test]
fn the_page_before_the_trial_has_started() {
    both_languages(Entitlement::TrialNotStarted, "settings_full_version_new");
}

#[test]
fn the_page_during_the_trial() {
    both_languages(
        Entitlement::Trial { days_left: 2 },
        "settings_full_version_trial",
    );
}

#[test]
fn the_page_after_the_trial() {
    both_languages(Entitlement::Expired, "settings_full_version_expired");
}

#[test]
fn the_page_after_the_purchase() {
    both_languages(
        Entitlement::Full(FullKind::Purchased),
        "settings_full_version_full",
    );
}

#[test]
fn the_page_of_an_earlier_buyer() {
    both_languages(
        Entitlement::Full(FullKind::EarlierPurchase),
        "settings_full_version_earlier",
    );
}

/// The picture for the App Store review: Marc's window size.
#[test]
fn the_review_picture_of_the_page_before_the_trial() {
    let app = page(Entitlement::TrialNotStarted, Language::English);
    let mut review = Simulator::with_size(
        iced::Settings {
            default_font: ncrs_ui_kit::APP_FONT,
            ..iced::Settings::default()
        },
        iced::Size::new(1740.0, 1130.0),
        app.view(),
    );
    matches_snapshot(&mut review, "settings_full_version_review_1740x1130");
}

fn main_window_matches(app: &mut App, name: &str) {
    const MARCS_WINDOW: iced::Size = iced::Size::new(1740.0, 1130.0);
    drop(app.update(crate::messages::Message::WindowResized(MARCS_WINDOW)));
    let mut window = Simulator::with_size(
        iced::Settings {
            default_font: ncrs_ui_kit::APP_FONT,
            ..iced::Settings::default()
        },
        MARCS_WINDOW,
        app.view(),
    );
    matches_snapshot(&mut window, name);
}

#[test]
fn the_error_bar_after_f5_in_an_expired_trial() {
    let mut app = App::with_fixed_panels()
        .in_language(Language::English)
        .with_entitlement(Entitlement::Expired);
    drop(app.update(crate::messages::Message::Transfer(
        crate::messages::TransferKind::Copy,
    )));
    main_window_matches(&mut app, "main_window_trial_expired_f5_en");
}

#[test]
fn the_reminder_in_the_status_bar_two_days_before_the_trial_ends() {
    let mut app = App::with_fixed_panels()
        .in_language(Language::English)
        .with_entitlement(Entitlement::Trial { days_left: 2 });
    main_window_matches(&mut app, "main_window_trial_2_days_en");
}
