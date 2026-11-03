//! The orders, their previews and the patterns, on readings built by hand.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::*;

fn bank_reading() -> FolderReading {
    let mut files = Vec::new();
    let mut seed = 0;
    for (bank, sender) in [("Sparkasse", "Sparkasse Chemnitz"), ("DKB", "DKB")] {
        for year in [2020, 2021] {
            for month in [1, 2, 3] {
                seed += 1;
                files.push(plan_file(
                    &format!("Bank/{bank}/{year}/{year}-{month:02}-01 {bank} Auszug {seed}.pdf"),
                    seed,
                    Some(sender),
                    date(year, month, 1),
                ));
            }
        }
    }
    reading("/archive", files)
}

fn preview(setup: &Setup, word: &str) -> Option<ChangePreview> {
    match setup.access.run(ChatWork::PreviewChange {
        reading: Arc::new(bank_reading()),
        kind_word: word.to_owned(),
        order: ChangeOrder::ByMonth,
    }) {
        ChatResult::Preview(preview) => preview,
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_preview_shows_the_newest_document_with_its_month_and_the_scope() {
    let preview = preview(&Setup::new(), "Rechnungen").unwrap();
    assert_eq!(preview.kind_label, "Invoice");
    assert_eq!(preview.change.area, PathBuf::from("Bank"));
    assert_eq!(preview.change.depth, 2);
    assert_eq!(preview.highlighted, 3);
    assert_eq!(
        preview.example,
        ["Bank", "DKB", "2021", "03", "2021-03-01 DKB Auszug 12.pdf"]
    );
    let scope = preview.scope.unwrap();
    assert_eq!(scope.values, ["DKB", "Sparkasse"]);
}

#[test]
fn a_word_that_contains_the_name_of_an_area_changes_all_of_that_area() {
    let preview = preview(&Setup::new(), "bankstatements").unwrap();
    assert_eq!(preview.change.area, PathBuf::from("Bank"));
    assert!(preview.change.kind_names.is_empty());
    assert_eq!(preview.example[3], "03");
}

#[test]
fn no_preview_for_a_word_that_names_no_kind_in_the_folder() {
    assert_eq!(preview(&Setup::new(), "Verträge"), None);
    assert_eq!(preview(&Setup::new(), "Wetter"), None);
}

fn ordered_reading(layout: fn(&str, i32, u32) -> String) -> FolderReading {
    let mut files = Vec::new();
    let mut seed = 0;
    for (bank, sender) in [("Sparkasse", "Sparkasse Chemnitz"), ("DKB", "DKB")] {
        for year in [2020, 2021] {
            for month in [1, 2, 3] {
                seed += 1;
                files.push(plan_file(
                    &layout(bank, year, month),
                    seed,
                    Some(sender),
                    date(year, month, 1),
                ));
            }
        }
    }
    reading("/archive", files)
}

fn preview_of(reading: FolderReading, order: ChangeOrder) -> ChangePreview {
    match Setup::new().access.run(ChatWork::PreviewChange {
        reading: Arc::new(reading),
        kind_word: "Rechnungen".to_owned(),
        order,
    }) {
        ChatResult::Preview(Some(preview)) => preview,
        other => panic!("{other:?}"),
    }
}

#[test]
fn every_order_previews_one_document_and_applies_to_a_filed_name() {
    let by_year = preview_of(
        ordered_reading(|bank, _, month| format!("Bank/{bank}/Auszug {month} {bank}.pdf")),
        ChangeOrder::ByYear,
    );
    assert_eq!(by_year.change.order, ChangeOrder::ByYear);
    assert_eq!(by_year.example[1], "2021");
    assert_eq!(by_year.highlighted, 1);
    let filed = change::apply_changes(
        PathBuf::from("Bank/DKB/a.pdf"),
        &fields(Some(date(2022, 7, 4)), Some("invoice"), Some("DKB")),
        &kinds(),
        std::slice::from_ref(&by_year.change),
    );
    assert_eq!(filed, PathBuf::from("Bank/2022/DKB/a.pdf"));

    let by_sender = preview_of(
        ordered_reading(|_, year, month| format!("Bank/{year}/Auszug {month}.pdf")),
        ChangeOrder::BySender,
    );
    assert_eq!(by_sender.example[1], "DKB");
    assert_eq!(by_sender.highlighted, 1);
    let filed = change::apply_changes(
        PathBuf::from("Bank/2022/a.pdf"),
        &fields(Some(date(2022, 7, 4)), Some("invoice"), Some("DKB")),
        &kinds(),
        std::slice::from_ref(&by_sender.change),
    );
    assert_eq!(filed, PathBuf::from("Bank/DKB/2022/a.pdf"));

    let by_kind = preview_of(
        ordered_reading(|_, year, month| format!("Bank/{year}/Auszug {month}.pdf")),
        ChangeOrder::ByKind,
    );
    assert_eq!(by_kind.example[1], "Invoice");
    let filed = change::apply_changes(
        PathBuf::from("Bank/2022/a.pdf"),
        &fields(Some(date(2022, 7, 4)), Some("invoice"), None),
        &kinds(),
        std::slice::from_ref(&by_kind.change),
    );
    assert_eq!(filed, PathBuf::from("Bank/Invoice/2022/a.pdf"));

    let flat = preview_of(
        ordered_reading(|bank, year, month| format!("Bank/{bank}/{year}/Auszug {month}.pdf")),
        ChangeOrder::Flat,
    );
    assert_eq!(flat.example.len(), 2);
    assert_eq!(flat.highlighted, 1);
    let filed = change::apply_changes(
        PathBuf::from("Bank/DKB/2022/a.pdf"),
        &fields(Some(date(2022, 7, 4)), Some("invoice"), Some("DKB")),
        &kinds(),
        std::slice::from_ref(&flat.change),
    );
    assert_eq!(filed, PathBuf::from("Bank/a.pdf"));
}

#[test]
fn an_order_the_folder_has_already_changes_nothing() {
    let by_year =
        ordered_reading(|bank, year, month| format!("Bank/{bank}/{year}/Auszug {month}.pdf"));
    let preview = match Setup::new().access.run(ChatWork::PreviewChange {
        reading: Arc::new(by_year),
        kind_word: "Rechnungen".to_owned(),
        order: ChangeOrder::ByYear,
    }) {
        ChatResult::Preview(preview) => preview,
        other => panic!("{other:?}"),
    };
    assert_eq!(preview, None);
    let change = OrderChange {
        area: PathBuf::from("Bank"),
        depth: 1,
        order: ChangeOrder::BySender,
        only: None,
        kind_names: Vec::new(),
        pattern: None,
    };
    let named = PathBuf::from("Bank/DKB/a.pdf");
    assert_eq!(
        change::apply_changes(
            named.clone(),
            &fields(None, None, Some("DKB")),
            &kinds(),
            &[change]
        ),
        named
    );
}

fn pattern_preview_of(reading: FolderReading, levels: Vec<PatternLevel>) -> Option<PatternPreview> {
    match Setup::new().access.run(ChatWork::PreviewPattern {
        reading: Arc::new(reading),
        levels,
    }) {
        ChatResult::PatternPreviewed(preview) => preview,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_pattern_replaces_the_folders_below_every_area_and_counts_what_moves() {
    let reading =
        ordered_reading(|bank, year, month| format!("Bank/{bank}/{year}/Auszug {month}.pdf"));
    let choices = pattern_choices(&reading.structure);
    assert!(
        choices.iter().all(|choice| !choice.levels.is_empty()),
        "{choices:?}"
    );

    let preview =
        pattern_preview_of(reading, vec![PatternLevel::Year, PatternLevel::Month]).unwrap();
    assert_eq!(preview.moved, 12);
    assert_eq!(preview.before.len(), 4);
    assert_eq!(preview.after.len(), 4);
    assert_eq!(preview.after[1], "2021");
    assert_eq!(preview.after[2], "03");

    let moved = change::apply_changes(
        PathBuf::from("Bank/DKB/2022/a.pdf"),
        &fields(Some(date(2022, 7, 4)), Some("invoice"), Some("DKB")),
        &kinds(),
        std::slice::from_ref(&preview.change),
    );
    assert_eq!(moved, PathBuf::from("Bank/2022/07/a.pdf"));

    let without_date = change::apply_changes(
        PathBuf::from("Bank/DKB/2022/a.pdf"),
        &fields(None, Some("invoice"), Some("DKB")),
        &kinds(),
        std::slice::from_ref(&preview.change),
    );
    assert_eq!(
        without_date,
        PathBuf::from("Bank/DKB/2022/a.pdf"),
        "a document without a date stays"
    );
    let elsewhere = change::apply_changes(
        PathBuf::from("Other/x/a.pdf"),
        &fields(Some(date(2022, 7, 4)), None, None),
        &kinds(),
        std::slice::from_ref(&preview.change),
    );
    assert_eq!(elsewhere, PathBuf::from("Other/x/a.pdf"));
}

#[test]
fn a_pattern_everything_follows_has_nothing_to_preview() {
    let reading = ordered_reading(|_, year, month| format!("Bank/{year}/Auszug {month}.pdf"));
    assert_eq!(pattern_preview_of(reading, vec![PatternLevel::Year]), None);
}

#[test]
fn a_target_without_neighbours_files_by_the_layout_it_was_given() {
    let setup = Setup::new();
    let source = reading(
        "/inbox",
        vec![plan_file(
            "a.pdf",
            1,
            Some("Sparkasse Chemnitz"),
            date(2021, 3, 1),
        )],
    );
    let run = |layout: Option<Vec<PatternLevel>>| match setup.access.run(ChatWork::Trial {
        source: Arc::new(source.clone()),
        target: Arc::new(reading("/new", Vec::new())),
        changes: Vec::new(),
        answered: AnsweredGroups::default(),
        layout,
    }) {
        ChatResult::Trial(plan) => plan.unwrap().entries.remove(0).outcome,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        run(Some(vec![PatternLevel::Year])),
        TrialOutcome::Filed {
            to: PathBuf::from("2021/a.pdf")
        }
    );
    assert_eq!(
        run(Some(Vec::new())),
        TrialOutcome::Filed {
            to: PathBuf::from("a.pdf")
        }
    );
    assert_eq!(
        run(None),
        TrialOutcome::Stays(StayReason::NoNeighbours),
        "without a layout nothing is guessed"
    );
}
