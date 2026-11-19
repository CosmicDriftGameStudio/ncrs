use super::app_simulator as simulator;
use std::path::PathBuf;

use super::*;
use crate::messages::{SavedQueriesMessage, SettingsMessage};
use crate::saved_queries::fixtures::{hit, loaded_list, THIS_YEAR};
use crate::saved_queries::{Draft, SavedQueries};

fn queries_message(message: SavedQueriesMessage) -> Message {
    Message::SavedQueries(message)
}

fn shows(list: SavedQueries, label: &str) -> bool {
    simulator(&App::with_saved_queries(list))
        .find(label)
        .is_ok()
}

/// The first query selected and run: two files to copy, one missing.
fn after_run() -> SavedQueries {
    let mut list = loaded_list();
    let (serial, _) = list.begin_run().expect("a spec");
    let priced = |name: &str, missing: bool, minor_units: i64| {
        let mut found = hit(name, missing);
        found.fields.sender = Some("Company A".to_owned());
        found.fields.date = chrono::NaiveDate::from_ymd_opt(2024, 3, 9);
        found.fields.amount = Some(ncrs_index::Amount {
            minor_units,
            currency: ncrs_index::Currency::new("EUR").expect("a currency"),
        });
        found
    };
    list.ran(
        serial,
        vec![
            priced("invoice-1.pdf", false, 123_450),
            hit("invoice-2.pdf", false),
            priced("invoice-3.pdf", true, 5_000),
        ],
    );
    list
}

fn at_export() -> SavedQueries {
    let mut list = after_run();
    list.start_export(PathBuf::from("/Archive/Export"));
    list
}

/// The reports with the hits each one finds today.
fn listing() -> SavedQueries {
    let mut list = loaded_list();
    let ids: Vec<_> = list.rows().iter().map(|query| query.id).collect();
    list.counted(ids.into_iter().zip([12, 3]).collect());
    list
}

fn editing() -> SavedQueries {
    let mut list = after_run();
    list.begin_edit(0);
    list
}

fn asking_to_delete() -> SavedQueries {
    let mut list = loaded_list();
    list.select(0);
    list.ask_delete();
    list
}

#[test]
fn the_button_of_the_remove_question_removes_the_report() {
    let app = App::with_saved_queries(asking_to_delete());
    let mut sim = simulator(&app);
    sim.click(button_repeating_the_title("Remove report"))
        .expect("the confirm button");
    assert!(sim.into_messages().any(|message| matches!(
        message,
        Message::SavedQueries(SavedQueriesMessage::ConfirmDelete)
    )));
}

fn empty_list() -> SavedQueries {
    let mut list = SavedQueries::new(THIS_YEAR);
    list.loaded(Vec::new());
    list
}

fn click_sends(list: SavedQueries, label: &str, message: SavedQueriesMessage) {
    let app = App::with_saved_queries(list);
    let mut sim = simulator(&app);
    sim.click(label).expect(label);
    assert_eq!(
        sim.into_messages().map(without_focus).collect::<Vec<_>>(),
        [queries_message(message)],
        "{label}"
    );
}

#[test]
fn the_page_lists_the_reports_with_their_criteria_and_the_buttons_in_the_row() {
    for label in [
        "Company A 2024",
        "Taxes 2023",
        "12 hits",
        "Change",
        "Export …",
        "Remove",
    ] {
        assert!(shows(listing(), label), "{label}");
    }
}

#[test]
fn an_empty_list_says_so() {
    assert!(shows(
        empty_list(),
        "No reports yet. Choose + Report to make one, or start from a template."
    ));
}

#[test]
fn the_buttons_of_a_row_send_what_they_say_for_that_row() {
    click_sends(listing(), "Change", SavedQueriesMessage::Edit(0));
    click_sends(listing(), "Export …", SavedQueriesMessage::StartExport(0));
    click_sends(listing(), "Remove", SavedQueriesMessage::AskDelete(0));
}

#[test]
fn the_change_dialog_shows_name_criteria_and_result_and_hands_the_report_to_the_search() {
    for label in [
        "Edit in search",
        "RESULT",
        "Hits",
        "View list",
        "Cancel",
        "Save",
    ] {
        assert!(shows(editing(), label), "{label}");
    }
    click_sends(
        editing(),
        "Edit in search",
        SavedQueriesMessage::EditInSearch,
    );
    click_sends(editing(), "View list", SavedQueriesMessage::ToggleMissing);
    click_sends(editing(), "Cancel", SavedQueriesMessage::CancelStep);
}

#[test]
fn the_list_stays_the_same_while_a_dialog_is_up() {
    for list in [editing(), at_export(), asking_to_delete()] {
        assert!(shows(list.clone(), "Taxes 2023"));
        assert!(shows(list, "Change"));
    }
}

#[test]
fn the_export_step_says_what_happens_before_anything_is_copied() {
    for label in [
        "Target folder",
        "/Archive/Export",
        "Files to copy",
        "Copies only; moves and deletes nothing. Existing files stay untouched.",
        "Choose folder …",
        "Cancel",
    ] {
        assert!(shows(at_export(), label), "{label}");
    }
    click_sends(
        at_export(),
        "Choose folder …",
        SavedQueriesMessage::PickFolder,
    );
}

#[test]
fn deleting_asks_in_a_dialog_first() {
    assert!(shows(
        asking_to_delete(),
        "Remove the report ‘Company A 2024’? Documents stay as they are."
    ));
}

#[test]
fn the_view_is_worded_in_german_too() {
    let app = App::with_saved_queries(at_export()).in_language(Language::German);
    let mut sim = simulator(&app);
    for label in [
        "Abbrechen",
        "Zielordner",
        "Ordner wählen …",
        "Kopiert nur, verschiebt und löscht nichts. Vorhandene Dateien bleiben unangetastet.",
    ] {
        assert!(sim.find(label).is_ok(), "{label}");
    }
}

#[test]
fn escape_closes_the_dialog_first_and_then_the_settings() {
    let mut app = App::with_saved_queries(at_export());
    let _ = app.update(Message::Settings(SettingsMessage::Close));
    let page = app.saved_queries_for_test().unwrap();
    assert!(page.export_step().is_none());
    assert!(page.results().is_some());
    assert!(app.open_hermes_page().is_some());

    let _ = app.update(Message::Settings(SettingsMessage::Close));
    assert!(app.open_hermes_page().is_none());
}

fn matches_snapshot(list: SavedQueries, name: &str) {
    let language = if name.contains("_de") {
        Language::German
    } else {
        Language::English
    };
    let app = App::with_saved_queries(list).in_language(language);
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
fn the_view_matches_its_snapshots() {
    for language in ["en", "de"] {
        matches_snapshot(listing(), &format!("saved_queries_{language}"));
        matches_snapshot(empty_list(), &format!("saved_queries_empty_{language}"));
        matches_snapshot(editing(), &format!("saved_queries_edit_{language}"));
        matches_snapshot(at_export(), &format!("saved_queries_export_{language}"));
        matches_snapshot(
            asking_to_delete(),
            &format!("saved_queries_delete_{language}"),
        );
    }
}

/// A search handed over to be saved: a dialog for a report not stored yet.
fn unsaved_list() -> SavedQueries {
    let mut list = SavedQueries::new(THIS_YEAR);
    list.prefill(Draft {
        name: String::new(),
        spec: ncrs_index::QuerySpec {
            year: Some(THIS_YEAR),
            full_text: Some("telekom".to_owned()),
            ..ncrs_index::QuerySpec::default()
        },
    });
    list.loaded(crate::saved_queries::fixtures::stored_queries());
    let (serial, _) = list.begin_run().expect("a spec");
    list.ran(serial, vec![hit("invoice-1.pdf", false)]);
    list
}

#[test]
fn a_search_handed_over_opens_the_dialog_with_a_save_button() {
    click_sends(unsaved_list(), "Save", SavedQueriesMessage::Save);
}

#[test]
fn editing_in_the_search_binds_the_report() {
    let runtime = tokio::runtime::Runtime::new().expect("a runtime");
    let _inside = runtime.enter();
    let first = crate::saved_queries::fixtures::stored_queries()[0].id;
    let mut app = App::with_saved_queries(loaded_list()).with_ready_hermes_for_test();

    drop(app.update(queries_message(SavedQueriesMessage::EditInSearch)));

    assert!(app.search_is_open_for_test());
    assert!(app.saved_queries_for_test().is_none());
    let filters = app
        .search_for_test()
        .and_then(|view| view.filters.as_ref())
        .expect("the filters of an indexed search");
    assert_eq!(filters.report(), Some(first));
    assert_eq!(filters.selection().year, Some(2024));
}

#[test]
fn the_unsaved_row_matches_its_snapshot() {
    matches_snapshot(unsaved_list(), "saved_queries_unsaved_de");
}
