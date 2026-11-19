use super::app_simulator as simulator;
use std::collections::BTreeSet;

use ncrs_index::{Condition, ConditionKind, RuleConditions, RuleSpec};

use super::*;
use crate::config::Filing;
use crate::file_list_tags::TagExceptions;
use crate::messages::{HermesPage, SettingsFeature, SettingsMessage, TagsMessage};
use crate::tags_page::fixtures::loaded_page;
use crate::tags_page::TagsPage;
use crate::tags_work::{TagsFailure, TagsLoaded};

fn shows(page: TagsPage, label: &str) -> bool {
    simulator(&App::with_tags(page)).find(label).is_ok()
}

fn counted(tags: &[(&str, u64)]) -> Vec<(String, u64)> {
    tags.iter()
        .map(|(tag, count)| ((*tag).to_owned(), *count))
        .collect()
}

/// The tags and counts of the design board.
fn board_page() -> TagsPage {
    let mut page = TagsPage::new(ncrs_index::Kinds::built_in(
        ncrs_index::KindLanguage::English,
    ));
    let mut senders = counted(&[("telekom", 48), ("stadtwerke-musterhausen", 24)]);
    senders.extend((1..=38).map(|n| (format!("absender-{n}"), 1)));
    page.loaded(TagsLoaded {
        tally: ncrs_index::TagTally {
            own: counted(&[("steuer", 23), ("offen", 3), ("privat", 8)]),
            kind: counted(&[("invoice", 412), ("contract", 31), ("notice", 12)]),
            sender: senders,
            recipient: counted(&[("firma-a-gmbh", 9), ("max-muster", 4)]),
            year: counted(&[
                ("2026", 40),
                ("2025", 61),
                ("2024", 58),
                ("2023", 44),
                ("2022", 30),
                ("2021", 12),
            ]),
        },
        exceptions: TagExceptions::from([("notice".to_owned(), false)]),
    });
    page
}

fn with_sender_group_open() -> TagsPage {
    let mut page = board_page();
    page.toggle_open(crate::file_list_tags::TagGroup::Sender);
    page
}

fn renaming() -> TagsPage {
    let mut page = with_sender_group_open();
    page.start_rename("steuer".to_owned());
    page
}

fn refused_delete() -> TagsPage {
    let mut page = with_sender_group_open();
    page.ask_delete("steuer".to_owned());
    assert_eq!(page.begin_delete().as_deref(), Some("steuer"));
    page.failed(TagsFailure::InUse(vec![RuleSpec {
        name: None,
        conditions: RuleConditions::all_of(vec![Condition::new(
            ConditionKind::Sender,
            "Company A",
        )]),
        extract_number: None,
        extract_date: None,
        extract_amount: None,
        kind: Some("Invoice".to_owned()),
        tags: BTreeSet::from(["steuer".to_owned()]),
        filing_pattern: "{year}/{sender}".to_owned(),
        confirmed: true,
        enabled: true,
    }]));
    page
}

#[test]
fn the_page_shows_the_switch_and_the_tags() {
    for label in [
        "Tags",
        "Show tags",
        "At most 3 per file, then “+1”. Applies to both file lists.",
        "HERMES",
        "steuer",
        "invoice",
        "… and 38 more",
    ] {
        assert!(shows(with_sender_group_open(), label), "{label}");
    }
}

#[test]
fn the_german_page_uses_the_plain_words() {
    let app = App::with_tags(with_sender_group_open()).in_language(Language::German);
    let mut sim = simulator(&app);
    for label in [
        "Schlagworte",
        "Schlagwort suchen",
        "HERMES",
        "… und 38 weitere",
    ] {
        assert!(sim.find(label).is_ok(), "{label}");
    }
}

#[test]
fn a_search_that_finds_nothing_says_so() {
    let mut page = loaded_page();
    page.set_search("nothing like it".to_owned());
    assert!(shows(page, "No tag matches."));
}

#[test]
fn the_button_of_the_remove_question_removes_the_tag() {
    let mut asking = loaded_page();
    asking.ask_delete("steuer".to_owned());
    let app = App::with_tags(asking);
    let mut sim = simulator(&app);
    sim.click(button_repeating_the_title("Remove tag"))
        .expect("the confirm button");
    assert!(sim
        .into_messages()
        .any(|message| matches!(message, Message::Tags(TagsMessage::ConfirmDelete))));
}

#[test]
fn the_delete_question_has_its_button_and_a_refused_delete_names_the_rules() {
    assert!(renaming().is_editing());
    let mut asking = loaded_page();
    asking.ask_delete("steuer".to_owned());
    assert!(shows(asking, "Remove tag"));
    assert!(shows(
        refused_delete(),
        "‘steuer’ is a condition of these rules and stays: If the sender is “Company A” → Invoice …. Change the rules first."
    ));
}

#[test]
fn a_sidebar_click_opens_the_page_and_closes_the_other() {
    let mut app = App::with_saved_queries(crate::saved_queries::fixtures::loaded_list());
    let _ = app.update(Message::HermesPage(HermesPage::Tags));
    assert!(app.saved_queries_for_test().is_none());
    assert!(app.tags_for_test().is_some());
}

#[test]
fn escape_closes_the_dialog_first_and_then_the_settings() {
    let mut app = App::with_tags(renaming());
    let _ = app.update(Message::Settings(SettingsMessage::Close));
    assert!(app
        .tags_for_test()
        .is_some_and(|page| page.step().is_none()));
    assert!(app.open_hermes_page().is_some());
    let _ = app.update(Message::Settings(SettingsMessage::Close));
    assert!(app.open_hermes_page().is_none());
}

#[test]
fn the_work_done_sets_the_exceptions_the_file_list_follows() {
    let mut app = App::with_tags(loaded_page());
    let generation = app.tags_generation_for_test();
    let loaded = |exceptions: TagExceptions| TagsLoaded {
        exceptions,
        ..TagsLoaded::default()
    };
    let _ = app.update(Message::Tags(TagsMessage::Done {
        generation: generation.wrapping_add(1),
        result: Ok(loaded(TagExceptions::from([("old".to_owned(), true)]))),
    }));
    assert!(app.tag_exceptions_for_test().is_empty(), "a stale answer");

    let _ = app.update(Message::Tags(TagsMessage::Done {
        generation,
        result: Ok(loaded(TagExceptions::from([("steuer".to_owned(), false)]))),
    }));
    assert_eq!(
        app.tag_exceptions_for_test(),
        &TagExceptions::from([("steuer".to_owned(), false)])
    );
}

#[test]
fn the_settings_button_shows_the_tags_page_in_the_same_settings() {
    let mut app = App::with_settings_open(SettingsFeature::Look, Filing::default());
    let _ = app.update(Message::Settings(SettingsMessage::ChooseFileListTags));
    assert!(app.tags_for_test().is_some());
    assert_eq!(app.open_hermes_page(), Some(HermesPage::Tags));
    assert!(app.settings_for_test().is_some());
}

#[test]
fn the_look_page_has_the_switch_and_not_the_removed_ones() {
    let app = App::with_settings_open(SettingsFeature::Look, Filing::default());
    let mut sim = simulator(&app);
    assert!(sim.find("Show tags").is_ok(), "the switch");
    assert!(sim.find("Tags in the file list").is_err(), "removed row");
    assert!(sim.find("Which tags?").is_err(), "removed group");
    assert!(sim.find("Only show these").is_err(), "removed row");
}

fn matches_snapshot(page: TagsPage, name: &str) {
    let language = if name.ends_with("_de") {
        Language::German
    } else {
        Language::English
    };
    let app = App::with_tags(page).in_language(language);
    let matches = simulator(&app)
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image(format!("tests/snapshots/{name}.png"))
        .expect("snapshot comparison");
    assert!(
        matches,
        "the view does not match tests/snapshots/{name}.png"
    );
}

#[test]
fn the_open_menu_shows_its_entries_in_full() {
    let app = App::with_tags(with_sender_group_open()).in_language(Language::German);
    let mut sim = simulator(&app);
    sim.point_at(iced::Point::new(907.0, 247.0));
    let _ = sim.simulate(iced_test::simulator::click());
    let matches = sim
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image("tests/snapshots/menu_open_tags_de.png")
        .expect("snapshot comparison");
    assert!(matches, "the view does not match menu_open_tags_de.png");
}

#[test]
fn the_view_matches_its_snapshots() {
    for suffix in ["en", "de"] {
        matches_snapshot(with_sender_group_open(), &format!("hermes_tags_{suffix}"));
        matches_snapshot(renaming(), &format!("hermes_tags_rename_{suffix}"));
        matches_snapshot(refused_delete(), &format!("hermes_tags_in_use_{suffix}"));
    }
}
