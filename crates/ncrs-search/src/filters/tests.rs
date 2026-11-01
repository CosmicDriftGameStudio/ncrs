use std::path::PathBuf;

use ncrs_index::Amount;

use super::*;
use crate::test_support::{found_hit, six_hits};

fn search_for(words: &str) -> SearchFilters {
    let mut search = SearchFilters::new(Arc::default());
    search.ask(words);
    search
}

pub fn loaded_search() -> SearchFilters {
    let mut search = search_for("");
    search.ran(six_hits());
    search
}

fn names(search: &SearchFilters) -> Vec<String> {
    search
        .shown()
        .map(|hit| hit.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn the_list_is_newest_first_with_the_undated_last() {
    let search = loaded_search();

    assert_eq!(
        names(&search),
        ["b.pdf", "a.pdf", "d.pdf", "c.pdf", "e.pdf", "f.pdf"]
    );
}

#[test]
fn choosing_a_year_keeps_the_other_years_in_the_list_with_their_numbers() {
    let mut search = loaded_search();

    search.toggle(FacetValue::Year(2024));

    assert_eq!(names(&search), ["d.pdf", "c.pdf"]);
    assert_eq!(search.counts().years, [(2025, 2), (2024, 2), (2023, 1)]);
    search.toggle(FacetValue::Year(2025));
    assert_eq!(search.selection().year, Some(2025));
    assert_eq!(names(&search), ["b.pdf", "a.pdf"]);
    search.toggle(FacetValue::Year(2025));
    assert_eq!(search.selection().year, None);
    assert_eq!(search.shown.len(), 6);
}

#[test]
fn the_tags_count_over_what_is_left_and_a_chosen_one_stays_visible() {
    let mut search = loaded_search();
    assert_eq!(
        search.counts().tags,
        [
            ("office".to_owned(), 2),
            ("paid".to_owned(), 2),
            ("open".to_owned(), 1)
        ]
    );

    search.toggle(FacetValue::Year(2024));
    search.toggle(FacetValue::Tag("office".to_owned()));
    assert_eq!(names(&search), ["c.pdf"]);
    assert_eq!(search.counts().tags, [("office".to_owned(), 1)]);

    search.toggle(FacetValue::Year(2023));
    assert!(names(&search).is_empty());
    assert_eq!(search.counts().tags, [("office".to_owned(), 0)]);
}

#[test]
fn year_sender_and_tag_narrow_together_and_case_does_not_matter() {
    let mut search = loaded_search();

    search.toggle(FacetValue::Year(2025));
    search.toggle(FacetValue::Sender("FIRMA a".to_owned()));
    search.toggle(FacetValue::Tag("PAID".to_owned()));

    assert_eq!(names(&search), ["b.pdf"]);
    assert_eq!(search.counts().senders, [("firma a".to_owned(), 1)]);
}

#[test]
fn senders_and_kinds_are_counted_without_regard_to_case_and_the_most_first() {
    let search = loaded_search();

    assert_eq!(
        search.counts().senders,
        [("firma a".to_owned(), 3), ("Agency B".to_owned(), 2)]
    );
    assert_eq!(
        search.counts().kinds,
        [
            ("invoice".to_owned(), 3),
            ("contract".to_owned(), 1),
            ("letter".to_owned(), 1)
        ]
    );
}

#[test]
fn spellings_of_a_known_kind_are_one_facet_entry_and_one_choice() {
    let hits = vec![
        found_hit("a.pdf", None, None, Some("invoice"), &[]),
        found_hit("b.pdf", None, None, Some("Rechnung"), &[]),
        found_hit("c.pdf", None, None, Some("Steuerbescheid"), &[]),
    ];
    let mut selection = FacetSelection::default();

    assert_eq!(
        facet_counts(&hits, &selection, &KindCatalog::built_in()).kinds,
        [("invoice".to_owned(), 2), ("Steuerbescheid".to_owned(), 1)]
    );
    selection.toggle(
        FacetValue::Kind("Rechnung".to_owned()),
        &KindCatalog::built_in(),
    );
    let found: Vec<&str> = hits
        .iter()
        .filter(|hit| selection.admits_hit(hit, &KindCatalog::built_in()))
        .filter_map(|hit| hit.path.file_name()?.to_str())
        .collect();

    assert_eq!(found, ["a.pdf", "b.pdf"]);
    assert!(selection.holds(
        &FacetValue::Kind("Invoice".to_owned()),
        &KindCatalog::built_in()
    ));
    selection.toggle(
        FacetValue::Kind("invoice".to_owned()),
        &KindCatalog::built_in(),
    );
    assert!(selection.is_empty());
}

#[test]
fn the_words_of_a_custom_kind_are_one_facet_entry_and_one_choice() {
    let (catalog, skips) = KindCatalog::built_in().extended(&[ncrs_index::CustomKind {
        id: "payslip".to_owned(),
        german: Some("Lohnabrechnung".to_owned()),
        english: None,
        words: vec!["Gehaltsabrechnung".to_owned()],
    }]);
    assert!(skips.is_empty());
    let hits = vec![
        found_hit("a.pdf", None, None, Some("payslip"), &[]),
        found_hit("b.pdf", None, None, Some("Gehaltsabrechnung"), &[]),
    ];
    let mut selection = FacetSelection::default();

    assert_eq!(
        facet_counts(&hits, &selection, &catalog).kinds,
        [("payslip".to_owned(), 2)]
    );
    selection.toggle(FacetValue::Kind("Lohnabrechnung".to_owned()), &catalog);
    assert_eq!(selection.kind.as_deref(), Some("payslip"));
    assert!(hits.iter().all(|hit| selection.admits_hit(hit, &catalog)));
}

#[test]
fn a_value_of_a_single_valued_facet_replaces_the_chosen_one() {
    let mut selection = FacetSelection::default();

    selection.toggle(
        FacetValue::Kind("invoice".to_owned()),
        &KindCatalog::built_in(),
    );
    selection.toggle(
        FacetValue::Kind("contract".to_owned()),
        &KindCatalog::built_in(),
    );
    selection.toggle(FacetValue::Tag("a".to_owned()), &KindCatalog::built_in());
    selection.toggle(FacetValue::Tag("b".to_owned()), &KindCatalog::built_in());
    selection.toggle(FacetValue::Tag("A".to_owned()), &KindCatalog::built_in());
    selection.toggle(FacetValue::Sender("X".to_owned()), &KindCatalog::built_in());
    selection.toggle(
        FacetValue::Sender(" x ".to_owned()),
        &KindCatalog::built_in(),
    );

    assert_eq!(selection.kind.as_deref(), Some("contract"));
    assert_eq!(selection.tags, ["b".to_owned()].into());
    assert_eq!(selection.sender, None);
    assert!(!selection.is_empty());
    selection.toggle(
        FacetValue::Kind("Contract".to_owned()),
        &KindCatalog::built_in(),
    );
    selection.toggle(FacetValue::Tag("b".to_owned()), &KindCatalog::built_in());
    assert!(selection.is_empty());
}

#[test]
fn the_spec_holds_the_trimmed_words_and_the_choices() {
    let mut search = search_for("  rent office ");
    search.toggle(FacetValue::Year(2024));
    search.toggle(FacetValue::Tag("office".to_owned()));

    let spec = search.spec();

    assert_eq!(spec.full_text.as_deref(), Some("rent office"));
    assert_eq!(spec.year, Some(2024));
    assert_eq!(spec.tags, ["office".to_owned()].into());
    let (_, question) = search.ask("  rent office ");
    assert_eq!(
        question,
        QuerySpec {
            full_text: Some("rent office".to_owned()),
            ..QuerySpec::default()
        }
    );
    assert!(search.is_busy());
    assert_eq!(search_for("  ").spec().full_text, None);
}

#[test]
fn a_failure_ends_busy_and_is_shown_and_a_search_clears_it() {
    let mut search = search_for("");
    assert!(search.is_busy());

    search.failed("the index is not open".to_owned());

    assert!(!search.is_busy());
    assert_eq!(search.problem(), Some("the index is not open"));
    search.ask("rent");
    assert!(search.is_busy());
    assert_eq!(search.problem(), None);
}

fn marked(snippet: &Snippet) -> Vec<&str> {
    snippet
        .marks
        .iter()
        .map(|mark| &snippet.text[mark.clone()])
        .collect()
}

#[test]
fn a_snippet_loses_its_marks_and_keeps_where_they_stood() {
    let snippet = parse_snippet("…for Herr \u{2}Müller\u{3} and \u{2}Berlin\u{3}\nlines");

    assert_eq!(snippet.text, "…for Herr Müller and Berlin lines");
    assert_eq!(marked(&snippet), ["Müller", "Berlin"]);
}

#[test]
fn stray_marks_in_a_snippet_do_not_panic_or_leave_a_mark_behind() {
    let closing_first = parse_snippet("a\u{3}b \u{2}c\u{3}");
    assert_eq!(closing_first.text, "ab c");
    assert_eq!(marked(&closing_first), ["c"]);

    let never_closed = parse_snippet("one \u{2}two three");
    assert_eq!(never_closed.text, "one two three");
    assert_eq!(marked(&never_closed), ["two three"]);

    let opened_twice = parse_snippet("\u{2}a \u{2}b\u{3} c");
    assert_eq!(opened_twice.text, "a b c");
    assert_eq!(marked(&opened_twice), ["a b"]);

    let empty = parse_snippet("x\u{2}\u{3}y\u{2}");
    assert_eq!(empty.text, "xy");
    assert!(empty.marks.is_empty());

    assert_eq!(
        parse_snippet(""),
        Snippet {
            text: String::new(),
            marks: Vec::new()
        }
    );
}

#[test]
fn a_long_snippet_is_cut_around_its_first_match() {
    let snippet = parse_snippet(&format!(
        "{}\u{2}Müller\u{3}{}",
        "long ".repeat(20),
        " tail".repeat(20)
    ));

    let fitted = snippet.fitted(30);

    assert_eq!(fitted.text.chars().count(), 30);
    assert_eq!(marked(&fitted), ["Müller"]);
    assert_eq!(snippet.fitted(10_000), snippet);
}

#[test]
fn snippets_belong_to_the_hits_found_and_go_with_a_new_search() {
    let mut search = search_for("rent");
    search.ran(six_hits());
    let (paths, words) = search.snippet_request().unwrap();
    assert_eq!(paths.len(), 6);
    assert_eq!(words, "rent");

    search.add_snippets(vec![(
        PathBuf::from("archive/a.pdf"),
        "the \u{2}rent\u{3}".to_owned(),
    )]);
    let hit = search
        .hits
        .iter()
        .find(|hit| hit.path.ends_with("a.pdf"))
        .unwrap();
    assert_eq!(marked(search.snippet_of(hit).unwrap()), ["rent"]);

    search.ran(six_hits());
    assert!(search.snippets.is_empty());
    assert!(search_for("  ").snippet_request().is_none());
}

#[test]
fn only_words_or_a_chosen_value_can_be_saved_as_a_report() {
    let mut search = loaded_search();
    assert!(!search.can_save_as_report());
    search.toggle(FacetValue::Year(2024));
    assert!(search.can_save_as_report());
    search.clear(Facet::Year);
    search.ask("rent");
    assert!(search.can_save_as_report());
}

fn hit_for(
    name: &str,
    recipient: &str,
    kind: &str,
    own: &[&str],
    cents: i64,
    currency: &str,
) -> QueryHit {
    let mut hit = found_hit(name, Some((2026, 3, 9)), Some("Telekom"), Some(kind), own);
    hit.fields.recipient = Some(recipient.to_owned());
    hit.fields.amount = Some(Amount {
        minor_units: cents,
        currency: ncrs_index::Currency::new(currency).unwrap(),
    });
    hit
}

#[test]
fn the_recipient_narrows_counts_and_is_saved_as_the_tag_it_makes() {
    let mut search = search_for("");
    search.ran(vec![
        hit_for("a.pdf", "Firma A GmbH", "Rechnung", &["steuer"], 100, "EUR"),
        hit_for("b.pdf", "Marc privat", "Rechnung", &[], 250, "EUR"),
    ]);
    assert_eq!(
        search.counts().recipients,
        [
            ("Firma A GmbH".to_owned(), 1),
            ("Marc privat".to_owned(), 1)
        ]
    );

    search.toggle(FacetValue::Recipient("Firma A GmbH".to_owned()));

    assert_eq!(names(&search), ["a.pdf"]);
    assert!(search.spec().tags.contains("firma-a-gmbh"));
    assert_eq!(search.spec().year, None);
}

#[test]
fn a_chosen_tag_also_matches_the_automatic_and_the_more_specific_tags() {
    let mut search = search_for("");
    search.ran(vec![
        hit_for("a.pdf", "Firma A GmbH", "Rechnung", &[], 100, "EUR"),
        hit_for("b.pdf", "Firma AB", "Rechnung", &["steuer"], 100, "EUR"),
    ]);
    assert_eq!(search.counts().tags, [("steuer".to_owned(), 1)]);

    search.toggle(FacetValue::Tag("firma-a".to_owned()));
    assert_eq!(names(&search), ["a.pdf"]);
    search.toggle(FacetValue::Tag("firma-a".to_owned()));
    search.toggle(FacetValue::Tag("rechnung".to_owned()));
    assert_eq!(search.shown.len(), 2);
}

#[test]
fn the_total_is_only_given_for_one_currency() {
    let mut search = search_for("");
    search.ran(vec![
        hit_for("a.pdf", "X", "Rechnung", &[], 100, "EUR"),
        hit_for("b.pdf", "X", "Rechnung", &[], 250, "EUR"),
    ]);
    assert_eq!(search.total_amount().map(|sum| sum.minor_units), Some(350));

    search.ran(vec![
        hit_for("a.pdf", "X", "Rechnung", &[], 100, "EUR"),
        hit_for("b.pdf", "X", "Rechnung", &[], 250, "USD"),
    ]);
    assert_eq!(search.total_amount(), None);
}
