use super::*;
use crate::messages::ThemeFetchMessage;
use crate::open_vsx::FetchError;
use crate::theme_fetch::fixtures::fetched;
use crate::theme_fetch::{Fetched, ThemeFetch};
use crate::vsix::Brightness;

const HELP: &str =
    "Downloads from Open VSX, or a theme file from the https link you give. Nothing goes out before you press Enter or Fetch.";

fn fetch_message(message: ThemeFetchMessage) -> Message {
    Message::ThemeFetch(message)
}

fn two_themes() -> Fetched {
    let mut offered = fetched(&["Example Dark", "Example Light"]);
    offered.themes[1].brightness = Some(Brightness::Light);
    offered
}

fn choosing(offered: Fetched) -> ThemeFetch {
    let mut card = ThemeFetch::new();
    card.set_text("example.dark-themes".to_owned());
    card.start().unwrap();
    assert_eq!(card.finish_fetching(Ok(offered)), None);
    card
}

fn failed(text: &str, error: FetchError) -> ThemeFetch {
    let mut card = ThemeFetch::new();
    card.set_text(text.to_owned());
    card.fail(error);
    card
}

fn shows(card: ThemeFetch, label: &str) -> bool {
    simulator(&App::with_theme_fetch_card(card))
        .find(label)
        .is_ok()
}

fn messages_after_click(card: ThemeFetch, label: &str) -> Vec<Message> {
    let app = App::with_theme_fetch_card(card);
    let mut sim = simulator(&app);
    sim.click(label).expect("the clicked label");
    sim.into_messages().filter(is_action).collect()
}

#[test]
fn the_empty_card_asks_for_an_extension_and_says_where_it_goes() {
    for label in ["Fetch color theme", HELP, "Fetch", "Cancel"] {
        assert!(shows(ThemeFetch::new(), label), "{label}");
    }
    assert_eq!(
        messages_after_click(ThemeFetch::new(), "Fetch"),
        [fetch_message(ThemeFetchMessage::Activate)]
    );
    assert_eq!(
        messages_after_click(ThemeFetch::new(), "Cancel"),
        [fetch_message(ThemeFetchMessage::Close)]
    );
}

#[test]
fn the_card_is_worded_in_german_too() {
    let app = App::with_theme_fetch_card(ThemeFetch::new()).in_language(Language::German);
    let mut sim = simulator(&app);
    for label in ["Farbschema holen", "Holen", "Abbrechen"] {
        assert!(sim.find(label).is_ok(), "{label}");
    }
}

#[test]
fn a_complaint_is_shown_under_the_kept_text() {
    let invalid = failed("../etc", FetchError::InvalidId);
    assert!(shows(invalid, "Not a valid extension ID or link."));
    let unknown = failed(
        "example.dark-themes",
        FetchError::NotFound("example.dark-themes".to_owned()),
    );
    assert!(shows(
        unknown,
        "No extension example.dark-themes on Open VSX"
    ));
}

#[test]
fn while_it_downloads_the_card_says_so_and_cannot_start_again() {
    let mut card = ThemeFetch::new();
    card.set_text("example.dark-themes".to_owned());
    card.start().unwrap();
    assert!(shows(card.clone(), "Fetching from Open VSX…"));
    assert!(messages_after_click(card, "Fetch").is_empty());
}

#[test]
fn the_choice_names_the_extension_its_license_and_the_themes() {
    for label in [
        "Example Themes, version 1.2.3",
        "License: MIT",
        "Example Dark",
        "Example Light",
        "Cancel",
    ] {
        assert!(shows(choosing(two_themes()), label), "{label}");
    }
    assert!(!shows(choosing(two_themes()), "Fetch"), "nothing to fetch");
    assert_eq!(
        messages_after_click(choosing(two_themes()), "Example Light"),
        [fetch_message(ThemeFetchMessage::Pick(1))]
    );
}

#[test]
fn a_license_that_is_not_stated_says_so() {
    let mut offered = two_themes();
    offered.extension.license = None;
    assert!(shows(choosing(offered), "License: not stated"));
}

fn matches_snapshot(card: ThemeFetch, name: &str) {
    let language = if name.ends_with("_de") {
        Language::German
    } else {
        Language::English
    };
    let app = App::with_theme_fetch_card(card).in_language(language);
    let matches = simulator(&app)
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image(format!("tests/snapshots/{name}.png"))
        .expect("snapshot comparison");
    assert!(
        matches,
        "the theme card does not match tests/snapshots/{name}.png"
    );
}

#[test]
fn the_empty_card_matches_its_snapshot() {
    matches_snapshot(ThemeFetch::new(), "theme_fetch_input_en");
}

#[test]
fn the_choice_matches_its_snapshot() {
    matches_snapshot(choosing(two_themes()), "theme_fetch_choose_en");
}

#[test]
fn the_german_cards_match_their_snapshots() {
    matches_snapshot(ThemeFetch::new(), "theme_fetch_input_de");
    matches_snapshot(choosing(two_themes()), "theme_fetch_choose_de");
    matches_snapshot(
        failed(
            "example.dark-themes",
            FetchError::NotFound("example.dark-themes".to_owned()),
        ),
        "theme_fetch_error_de",
    );
}

#[test]
fn the_frame_grows_with_the_themes_and_keeps_its_width() {
    let one = frame_bounds(&App::with_theme_fetch_card(choosing(two_themes())));
    let names: Vec<String> = (0..10).map(|number| format!("Theme {number}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let ten = frame_bounds(&App::with_theme_fetch_card(choosing(fetched(&refs))));
    assert_eq!(one.width, ten.width);
    assert!(ten.height > one.height, "{one:?} {ten:?}");
}

#[test]
fn the_card_with_a_complaint_matches_its_snapshot() {
    matches_snapshot(
        failed(
            "example.dark-themes",
            FetchError::NotFound("example.dark-themes".to_owned()),
        ),
        "theme_fetch_error_en",
    );
}
