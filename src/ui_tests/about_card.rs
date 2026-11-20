use super::*;
use crate::messages::AboutMessage;
use ncrs_settings::about::AboutCard;

fn shows(app: &App, label: &str) -> bool {
    simulator(app).find(label).is_ok()
}

fn messages_after_click(label: &str) -> Vec<Message> {
    let app = App::with_about_card(AboutCard::new());
    let mut sim = simulator(&app);
    assert!(sim.click(label).is_ok(), "{label}");
    sim.into_messages().collect()
}

#[test]
fn the_card_shows_title_version_license_and_repository() {
    let app = App::with_about_card(AboutCard::new());
    for label in [
        "About ncrs",
        "Version 1.2.3",
        "MIT License",
        "https://github.com/CosmicDriftGameStudio/ncrs",
    ] {
        assert!(shows(&app, label), "{label}");
    }
}

#[test]
fn the_buttons_send_their_message_and_nothing_else() {
    assert_eq!(
        messages_after_click("Third-party licenses"),
        [Message::About(AboutMessage::OpenLicenses)]
    );
    assert_eq!(
        messages_after_click("Repository"),
        [Message::About(AboutMessage::OpenRepository)]
    );
    assert_eq!(
        messages_after_click("Close"),
        [Message::About(AboutMessage::Close)]
    );
}

#[test]
fn a_missing_licenses_file_names_the_online_copy_of_this_version() {
    let mut card = AboutCard::new();
    card.report_licenses_missing();
    let app = App::with_about_card(card);
    assert!(shows(
        &app,
        "The licence file is not part of this build. Read it online: https://github.com/CosmicDriftGameStudio/ncrs/releases/download/v1.2.3/THIRD_PARTY_LICENSES.html"
    ));
}

#[test]
fn the_card_is_worded_in_german_too() {
    let app = App::with_about_card(AboutCard::new()).in_language(Language::German);
    for label in [
        "Über ncrs",
        "Version 1.2.3",
        "MIT-Lizenz",
        "Lizenzen von Drittanbietern",
        "Schließen",
    ] {
        assert!(shows(&app, label), "{label}");
    }
}

fn matches_snapshot(language: Language, name: &str) {
    let app = App::with_about_card(AboutCard::new()).in_language(language);
    let matches = simulator(&app)
        .snapshot(&iced::Theme::Dark)
        .and_then(|snapshot| {
            snapshot.matches_reference_image(format!("tests/snapshots/{name}.png"))
        })
        .is_ok_and(|matches| matches);
    assert!(
        matches,
        "the about card does not match tests/snapshots/{name}.png"
    );
}

#[test]
fn the_card_matches_its_snapshots() {
    matches_snapshot(Language::English, "about_en");
    matches_snapshot(Language::German, "about_de");
    matches_snapshot(Language::EnglishFuturama, "about_en_futurama");
    matches_snapshot(Language::GermanFuturama, "about_de_futurama");
}

#[test]
fn only_the_futurama_variant_shows_the_theme_name_of_the_title() {
    let plain = App::with_about_card(AboutCard::new());
    assert!(!shows(&plain, "Planet Express crew roster"));
    let themed = App::with_about_card(AboutCard::new()).in_language(Language::EnglishFuturama);
    assert!(shows(&themed, "Planet Express crew roster"));
}
