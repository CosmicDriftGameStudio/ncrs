use super::*;
use crate::keymap::KeyAction;
use crate::keymap::KeyActionMessage as _;
use crate::messages::{QuickOpenMessage, SavedQueriesMessage, SearchMessage};
use crate::query_work::QueryHit;
use ncrs_index::{Amount, Currency};
use ncrs_search::filters::{Facet, FacetValue, SearchFilters};
use ncrs_search::test_support::found_hit;
use std::path::Path;

const MONTHS: [u32; 4] = [1, 2, 3, 4];

fn invoices_below(root: &Path) -> Vec<QueryHit> {
    MONTHS
        .iter()
        .map(|month| {
            let mut hit = found_hit(
                &format!("telekom-2026-{month:02}.pdf"),
                Some((2026, *month, 5)),
                Some("Telekom"),
                Some("invoice"),
                if *month == 4 { &["paid"] } else { &[] },
            );
            hit.path = root.join(format!("telekom-2026-{month:02}.pdf"));
            let file = crate::app::fixed_result_entry(
                hit.path.clone(),
                98_304 + u64::from(*month) * 2_048,
                (2026, *month, 6, 8, 30),
            );
            hit.stamp = file.modified.map(|mtime| ncrs_index::FileStamp {
                size: file.size,
                mtime,
            });
            hit.fields.amount = Some(Amount {
                minor_units: 10_320,
                currency: Currency::new("EUR").unwrap(),
            });
            hit
        })
        .collect()
}

fn filters_for(root: &Path, words: &str) -> SearchFilters {
    let mut filters = SearchFilters::new(Default::default());
    filters.ask(words);
    let mut hits = invoices_below(root);
    let mut elsewhere = found_hit("other.pdf", Some((2025, 5, 1)), None, None, &[]);
    elsewhere.path = root.join("other.pdf");
    hits.push(elsewhere);
    filters.ran(hits);
    filters
}

/// The search over four invoices read, the kind and the year chosen.
pub fn app_with_filters() -> App {
    let mut app = App::with_search_filters("telekom", |root| {
        let mut filters = filters_for(root, "telekom");
        filters.toggle(FacetValue::Kind("invoice".to_owned()));
        filters.toggle(FacetValue::Year(2026));
        filters.add_snippets(
            MONTHS
                .iter()
                .map(|month| {
                    (
                        root.join(format!("telekom-2026-{month:02}.pdf")),
                        format!("Mobile contract \u{2}Telekom\u{3} invoice {month}/2026"),
                    )
                })
                .collect(),
        );
        filters
    });
    let rows = app.search_rows_for_test();
    if let Some(view) = app.search_for_test_mut() {
        view.mirror_index(rows);
    }
    app
}

fn shows(app: &App, label: &str) -> bool {
    simulator(app).find(label).is_ok()
}

#[test]
fn the_filters_narrow_the_list_and_the_summary_counts_what_is_left() {
    let app = app_with_filters();

    let view = app.search_for_test().unwrap();
    assert_eq!(view.results.len(), 4);
    assert!(shows(&app, "Found by name and content: 4 files · 4 lines"));
    assert!(shows(&app, " · Sum 412.80 EUR"));
    assert!(shows(&app, "Save as report"));
    assert!(shows(
        &app,
        "Filters only apply to documents ncrs has read. Filters apply together (2026 and Invoice)."
    ));
}

#[test]
fn a_search_without_an_index_has_no_filters() {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::OpenSearch));

    assert!(app.search_for_test().unwrap().filters.is_none());
    assert!(!shows(&app, "Save as report"));
}

#[test]
fn choosing_and_clearing_a_value_switches_between_the_index_and_the_walk() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _inside = runtime.enter();
    let mut app = app_with_filters();

    drop(app.update(Message::Search(SearchMessage::ClearFacet(Facet::Year))));
    assert!(app.search_for_test().unwrap().is_filtered());
    drop(app.update(Message::Search(SearchMessage::ClearFacet(Facet::Kind))));
    assert!(!app.search_for_test().unwrap().is_filtered());

    drop(app.update(Message::Search(SearchMessage::ToggleFacet(
        FacetValue::Year(2025),
    ))));
    let view = app.search_for_test().unwrap();
    assert!(view.is_filtered());
    assert_eq!(view.results.len(), 1);
    assert!(view.results[0].path.ends_with("other.pdf"));
}

#[test]
fn an_answer_of_an_older_question_is_dropped() {
    let mut app = app_with_filters();
    let root = app.search_for_test().unwrap().root.clone();
    let stale = app
        .search_for_test()
        .unwrap()
        .filters
        .as_ref()
        .unwrap()
        .generation()
        .wrapping_sub(1);

    drop(app.update(Message::Search(SearchMessage::IndexRan {
        generation: stale,
        result: Ok(vec![found_hit("x.pdf", None, None, None, &[])]),
    })));

    assert_eq!(app.search_for_test().unwrap().results.len(), 4);
    assert!(app
        .search_for_test()
        .unwrap()
        .results
        .iter()
        .all(|result| result.path.starts_with(&root)));
}

#[test]
fn a_hit_outside_the_folder_or_the_name_filter_is_left_out() {
    let mut app = app_with_filters();
    let rows = app.search_rows_for_test();
    let view = app.search_for_test_mut().unwrap();
    view.names = "*-03.pdf".to_owned();

    view.mirror_index(rows);

    assert_eq!(view.results.len(), 1);
    view.names = String::new();
    view.exclude = "*-0[12].pdf".to_owned();
    view.mirror_index(rows);
    assert_eq!(view.results.len(), 2);
}

#[test]
fn save_as_report_opens_the_reports_page_with_a_new_report_of_the_search() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _inside = runtime.enter();
    let mut app = app_with_filters().with_ready_hermes_for_test();

    drop(app.update(Message::Search(SearchMessage::SaveAsReport)));

    assert!(!app.search_is_open_for_test());
    let generation = app.saved_queries_generation_for_test();
    drop(
        app.update(Message::SavedQueries(SavedQueriesMessage::Loaded {
            generation,
            result: Ok(Vec::new()),
        })),
    );
    let draft = app.saved_queries_for_test().unwrap().unsaved().unwrap();
    assert_eq!(draft.spec.full_text.as_deref(), Some("telekom"));
    assert_eq!(draft.spec.year, Some(2026));
    assert_eq!(draft.name, "");
}

#[test]
fn nothing_is_saved_as_a_report_without_words_or_a_chosen_value() {
    let mut app = App::with_search_filters("", |root| filters_for(root, ""));

    drop(app.update(Message::Search(SearchMessage::SaveAsReport)));

    assert!(app.search_is_open_for_test());
    assert!(app.saved_queries_for_test().is_none());
}

#[test]
fn a_search_opened_from_a_report_offers_to_update_it_and_returns_to_the_reports() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _inside = runtime.enter();
    let mut app = App::with_search_filters("telekom", |root| {
        let mut filters = filters_for(root, "telekom");
        let report = crate::saved_queries::fixtures::stored_queries()[0].id;
        filters.edit_report(report, &ncrs_index::QuerySpec::default());
        filters
    })
    .with_ready_hermes_for_test();
    assert!(shows(&app, "Update report"));
    assert!(!shows(&app, "Save as report"));

    drop(app.update(Message::Search(SearchMessage::ReportUpdated(Ok(())))));

    assert!(!app.search_is_open_for_test());
    assert!(app.saved_queries_for_test().is_some());
}

#[test]
fn a_report_that_could_not_be_updated_stays_on_the_search_with_the_reason() {
    let mut app = App::with_search_filters("telekom", |root| filters_for(root, "telekom"));

    drop(app.update(Message::Search(SearchMessage::ReportUpdated(Err(
        "there is no saved query 3".to_owned(),
    )))));

    assert!(app.search_is_open_for_test());
    assert!(shows(&app, "there is no saved query 3"));
}

#[test]
fn the_old_command_list_action_opens_the_normal_search_and_is_no_longer_offered() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _inside = runtime.enter();
    let mut app = App::with_fixed_panels().with_ready_hermes_for_test();

    drop(app.update(KeyAction::SearchDocuments.message()));

    assert!(app.search_is_open_for_test());
    assert!(!KeyAction::SearchDocuments.is_offered());
}

#[test]
fn the_last_row_of_quick_open_opens_the_search_with_the_text() {
    use crate::filing::{FoundDocument, TextSource};
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _inside = runtime.enter();
    let mut app = App::with_fixed_panels().with_ready_hermes_for_test();
    app.open_quick_open_for_test(&["src/config.rs"], "");
    app.type_into_quick_open_for_test("config");
    app.deliver_documents_for_test(Ok(vec![FoundDocument {
        path: std::path::PathBuf::from("/d/config-notes.txt"),
        read_by: TextSource::Text,
    }]));

    // One name hit, one document, then the row.
    drop(app.update(Message::QuickOpen(QuickOpenMessage::RowActivated(2))));

    assert!(app.quick_open_for_test().is_none());
    assert!(app.search_is_open_for_test());
    assert_eq!(app.search_for_test().unwrap().query, "config");
}

fn matches_snapshot(language: Language, name: &str) {
    let app = app_with_filters().in_language(language);
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
fn the_search_with_filters_matches_its_snapshots() {
    matches_snapshot(Language::English, "search_filters_en");
    matches_snapshot(Language::German, "search_filters_de");
}
