//! The orders, their previews and the patterns, on readings built by hand.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use ncrs_ai::{NameBlock, NameSpec};

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
        name: None,
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

fn kindless_reading(area: &str) -> FolderReading {
    let mut files = Vec::new();
    let mut seed = 0;
    for year in [2020, 2021] {
        for month in [1, 2, 3, 4] {
            seed += 1;
            let mut file = plan_file(
                &format!("{area}/{year}/{year}-{month:02}-01 Dokument {seed}.pdf"),
                seed,
                Some("Allianz"),
                date(year, month, 1),
            );
            file.fields.kind = None;
            files.push(file);
        }
    }
    reading("/archive", files)
}

fn preview_word(reading: FolderReading, word: &str, order: ChangeOrder) -> Option<ChangePreview> {
    match Setup::new().access.run(ChatWork::PreviewChange {
        name: None,
        reading: Arc::new(reading),
        kind_word: word.to_owned(),
        order,
    }) {
        ChatResult::Preview(preview) => preview,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_kind_word_that_is_also_an_area_changes_the_whole_area_when_no_document_has_the_kind() {
    let preview = preview_word(
        kindless_reading("Versicherung"),
        "Versicherung",
        ChangeOrder::BySender,
    )
    .unwrap();
    assert_eq!(preview.change.area, PathBuf::from("Versicherung"));
    assert!(preview.change.kind_names.is_empty());
    assert_eq!(preview.example[0], "Versicherung");
}

#[test]
fn a_kind_word_that_is_also_an_area_changes_the_whole_area_when_few_documents_have_the_kind() {
    let mut reading = kindless_reading("Versicherung");
    for file in reading.files.iter_mut().take(2) {
        file.fields.kind = Some("Versicherung".to_owned());
    }
    let preview = preview_word(reading, "Versicherung", ChangeOrder::BySender).unwrap();
    assert_eq!(preview.change.area, PathBuf::from("Versicherung"));
    assert!(preview.change.kind_names.is_empty());
}

#[test]
fn an_area_without_kinds_is_ordered_by_month_with_a_path_from_real_segments() {
    let preview = preview_word(kindless_reading("Bank"), "Bank", ChangeOrder::ByMonth).unwrap();
    assert_eq!(preview.change.area, PathBuf::from("Bank"));
    assert_eq!(
        preview.example,
        ["Bank", "2021", "04", "2021-04-01 Dokument 8.pdf"]
    );
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
        name: None,
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
    let filed_by_year = change::apply_changes(
        PathBuf::from("Bank/DKB/a.pdf"),
        &fields(Some(date(2022, 7, 4)), Some("invoice"), Some("DKB")),
        &kinds(),
        std::slice::from_ref(&by_year.change),
    );
    assert_eq!(filed_by_year, PathBuf::from("Bank/2022/DKB/a.pdf"));

    let by_sender = preview_of(
        ordered_reading(|_, year, month| format!("Bank/{year}/Auszug {month}.pdf")),
        ChangeOrder::BySender,
    );
    assert_eq!(by_sender.example[1], "DKB");
    assert_eq!(by_sender.highlighted, 1);
    let filed_by_sender = change::apply_changes(
        PathBuf::from("Bank/2022/a.pdf"),
        &fields(Some(date(2022, 7, 4)), Some("invoice"), Some("DKB")),
        &kinds(),
        std::slice::from_ref(&by_sender.change),
    );
    assert_eq!(filed_by_sender, PathBuf::from("Bank/DKB/2022/a.pdf"));

    let by_kind = preview_of(
        ordered_reading(|_, year, month| format!("Bank/{year}/Auszug {month}.pdf")),
        ChangeOrder::ByKind,
    );
    assert_eq!(by_kind.example[1], "Invoice");
    let filed_by_kind = change::apply_changes(
        PathBuf::from("Bank/2022/a.pdf"),
        &fields(Some(date(2022, 7, 4)), Some("invoice"), None),
        &kinds(),
        std::slice::from_ref(&by_kind.change),
    );
    assert_eq!(filed_by_kind, PathBuf::from("Bank/Invoice/2022/a.pdf"));

    let flat = preview_of(
        ordered_reading(|bank, year, month| format!("Bank/{bank}/{year}/Auszug {month}.pdf")),
        ChangeOrder::Flat,
    );
    assert_eq!(flat.example.len(), 2);
    assert_eq!(flat.highlighted, 1);
    let filed_flat = change::apply_changes(
        PathBuf::from("Bank/DKB/2022/a.pdf"),
        &fields(Some(date(2022, 7, 4)), Some("invoice"), Some("DKB")),
        &kinds(),
        std::slice::from_ref(&flat.change),
    );
    assert_eq!(filed_flat, PathBuf::from("Bank/a.pdf"));
}

#[test]
fn an_order_the_folder_has_already_changes_nothing() {
    let by_year =
        ordered_reading(|bank, year, month| format!("Bank/{bank}/{year}/Auszug {month}.pdf"));
    let preview = match Setup::new().access.run(ChatWork::PreviewChange {
        name: None,
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
        name: None,
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
        name: None,
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

#[test]
fn an_area_whose_documents_have_no_sender_yet_lacks_the_field_for_a_sender_order() {
    let mut without_sender = kindless_reading("Versicherung");
    for file in &mut without_sender.files {
        file.fields.sender = None;
    }
    let lacking = |reading: &FolderReading, word: &str, order| {
        area_lacks_fields(&kinds(), reading, word, order)
    };
    assert_eq!(
        lacking(&without_sender, "Versicherung", ChangeOrder::BySender).as_deref(),
        Some("Versicherung")
    );
    assert_eq!(
        lacking(&without_sender, "Versicherung", ChangeOrder::ByMonth),
        None
    );
    assert_eq!(
        lacking(
            &kindless_reading("Versicherung"),
            "Versicherung",
            ChangeOrder::BySender
        ),
        None
    );
    assert_eq!(
        lacking(&without_sender, "Wetter", ChangeOrder::BySender),
        None
    );
}

#[test]
fn a_branch_without_the_field_does_not_hide_a_branch_of_the_area_that_has_it() {
    let mut files = Vec::new();
    for (seed, folder, sender) in [
        (1, "Unfall", None),
        (2, "Unfall", None),
        (3, "Unfall", None),
    ]
    .into_iter()
    .chain([(4, "Reise", Some("Allianz")), (5, "Reise", Some("Allianz"))])
    {
        let mut file = plan_file(
            &format!("Versicherung/{folder}/2021-0{seed}-01 Dokument {seed}.pdf"),
            seed,
            sender,
            date(2021, u32::from(seed), 1),
        );
        file.fields.kind = None;
        file.fields.sender = sender.map(str::to_owned);
        files.push(file);
    }
    let preview = preview_word(
        reading("/archive", files),
        "Versicherung",
        ChangeOrder::BySender,
    );
    assert_eq!(preview.unwrap().change.area, PathBuf::from("Versicherung"));
}

fn name_of(blocks: &[NameBlock], separator: char) -> NameSpec {
    NameSpec {
        blocks: blocks.to_vec(),
        separator,
    }
}

fn names_only(spec: NameSpec) -> OrderChange {
    OrderChange {
        area: PathBuf::from("Bank"),
        depth: 1,
        order: ChangeOrder::FileNames,
        only: None,
        kind_names: Vec::new(),
        pattern: None,
        name: Some(spec),
    }
}

fn renamed(change: &OrderChange, path: &str, when: Option<NaiveDate>) -> PathBuf {
    change::apply_changes(
        PathBuf::from(path),
        &fields(when, Some("invoice"), Some("DKB")),
        &kinds(),
        std::slice::from_ref(change),
    )
}

#[test]
fn a_name_from_blocks_goes_in_front_of_the_old_name_once() {
    let change = names_only(name_of(
        &[NameBlock::Year, NameBlock::Month, NameBlock::Day],
        '_',
    ));
    let when = Some(date(2022, 7, 4));
    let once = renamed(&change, "Bank/DKB/Auszug.pdf", when);
    assert_eq!(once, PathBuf::from("Bank/DKB/2022_07_04_Auszug.pdf"));
    assert_eq!(renamed(&change, once.to_str().unwrap(), when), once);
    assert_eq!(
        renamed(&change, "Bank/DKB/2022-07-04 Auszug.pdf", when),
        once,
        "a date of the same day in front is replaced"
    );
    assert_eq!(
        renamed(&change, "Bank/DKB/Auszug.pdf", None),
        PathBuf::from("Bank/DKB/Auszug.pdf"),
        "without a date the name stays"
    );
    assert_eq!(
        renamed(&change, "Other/DKB/Auszug.pdf", when),
        PathBuf::from("Other/DKB/Auszug.pdf"),
        "another area is not touched"
    );
}

#[test]
fn the_separator_and_the_blocks_decide_the_name() {
    let change = names_only(name_of(&[NameBlock::Sender, NameBlock::Year], '-'));
    assert_eq!(
        renamed(&change, "Bank/DKB/Auszug.pdf", Some(date(2022, 7, 4))),
        PathBuf::from("Bank/DKB/DKB-2022-Auszug.pdf")
    );
    let only_date = names_only(name_of(&[NameBlock::Day], '.'));
    assert_eq!(
        renamed(&only_date, "Bank/DKB/Auszug", Some(date(2022, 7, 4))),
        PathBuf::from("Bank/DKB/04.Auszug")
    );
}

#[test]
fn a_names_only_preview_shows_and_highlights_the_new_file_name() {
    let spec = name_of(&[NameBlock::Year, NameBlock::Month, NameBlock::Day], '_');
    let result = Setup::new().access.run(ChatWork::PreviewChange {
        name: Some(spec),
        reading: Arc::new(bank_reading()),
        kind_word: "Bank".to_owned(),
        order: ChangeOrder::FileNames,
    });
    let ChatResult::Preview(Some(preview)) = result else {
        panic!("{result:?}");
    };
    assert_eq!(preview.highlighted, preview.example.len() - 1);
    assert!(
        preview.example.last().unwrap().starts_with("2021_03_01_"),
        "{:?}",
        preview.example
    );
}

#[test]
fn a_name_on_top_of_an_order_is_part_of_the_example() {
    let spec = name_of(&[NameBlock::Year], '_');
    let result = Setup::new().access.run(ChatWork::PreviewChange {
        name: Some(spec),
        reading: Arc::new(ordered_reading(|bank, _, month| {
            format!("Bank/{bank}/Auszug {month}.pdf")
        })),
        kind_word: "Bank".to_owned(),
        order: ChangeOrder::ByYear,
    });
    let ChatResult::Preview(Some(preview)) = result else {
        panic!("{result:?}");
    };
    let file = preview.example.last().unwrap();
    assert!(file.starts_with("2021_"), "{file}");
    assert_eq!(preview.example.len(), 4);
}

#[test]
fn an_area_ordered_that_way_already_still_gets_the_new_file_names() {
    let run = |name: Option<NameSpec>| {
        Setup::new().access.run(ChatWork::PreviewChange {
            name,
            reading: Arc::new(ordered_reading(|bank, year, month| {
                format!("Bank/{bank}/{year}/Auszug {month}.pdf")
            })),
            kind_word: "Bank".to_owned(),
            order: ChangeOrder::ByYear,
        })
    };
    let ChatResult::Preview(Some(preview)) = run(Some(name_of(&[NameBlock::Year], '_'))) else {
        panic!("expected a names preview");
    };
    assert_eq!(preview.change.order, ChangeOrder::FileNames);
    let file = preview.example.last().unwrap();
    assert!(file.starts_with("2021_"), "{file}");
    assert!(
        matches!(run(None), ChatResult::Preview(None)),
        "without a name there is nothing to change"
    );
}

#[test]
fn everywhere_with_only_a_name_counts_every_renamed_document() {
    let reading = bank_reading();
    let total = reading.files.len();
    let result = Setup::new().access.run(ChatWork::PreviewPattern {
        name: Some(name_of(&[NameBlock::Year], '_')),
        reading: Arc::new(reading),
        levels: Vec::new(),
    });
    let ChatResult::PatternPreviewed(Some(preview)) = result else {
        panic!("{result:?}");
    };
    assert_eq!(preview.moved, total);
    assert_ne!(preview.before.last(), preview.after.last());
}
