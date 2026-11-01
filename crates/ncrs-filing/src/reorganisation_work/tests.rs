use std::fs;
use std::path::Path;

use ncrs_index::{Condition, ConditionKind, PlanOutcome};

use super::fixtures::*;
use super::*;

#[test]
fn the_plan_has_an_entry_of_every_kind_and_a_target_for_the_ones_that_can_run() {
    let items = sample_items();

    assert!(matches!(
        item(&items, "Misc/2024-04-01_invoice.pdf").item.outcome,
        PlanOutcome::Move { .. }
    ));
    assert!(matches!(
        item(&items, "Insurance/2023/2024-05-01_invoice.pdf")
            .item
            .outcome,
        PlanOutcome::Move { .. }
    ));
    assert!(matches!(
        item(&items, "Insurance/2024/scan 0042.pdf").item.outcome,
        PlanOutcome::Rename { .. }
    ));
    assert!(matches!(
        item(&items, "Insurance/2023/2024-06-01_invoice.pdf")
            .item
            .outcome,
        PlanOutcome::Conflict { .. }
    ));
    assert!(matches!(
        item(&items, "Insurance/2024/IMG_7.pdf").item.outcome,
        PlanOutcome::NoAction(_)
    ));
}

#[test]
fn only_a_rule_the_user_has_not_confirmed_is_marked() {
    let items = sample_items();

    assert!(!item(&items, "Insurance/2024/2024-07-01_invoice.pdf").unconfirmed_rule());
    assert!(item(&items, "Insurance/2024/2024-08-01_invoice.pdf").unconfirmed_rule());
    assert!(!item(&items, "Misc/2024-04-01_invoice.pdf").unconfirmed_rule());
}

#[test]
fn a_misplaced_entry_carries_the_label_and_state_of_its_rule() {
    let items = sample_items();

    let confirmed = item(&items, "Insurance/2024/2024-07-01_invoice.pdf");
    let suggested = item(&items, "Insurance/2024/2024-08-01_invoice.pdf");

    let confirmed_rule = confirmed.rule.as_ref().unwrap();
    assert_eq!(
        (confirmed_rule.label.as_str(), confirmed_rule.confirmed),
        ("tax notice", true)
    );
    let suggested_rule = suggested.rule.as_ref().unwrap();
    assert_eq!(
        (suggested_rule.label.as_str(), suggested_rule.confirmed),
        ("late fee", false)
    );
    assert!(item(&items, "Misc/2024-04-01_invoice.pdf").rule.is_none());
}

#[test]
fn a_year_mismatch_carries_the_date_of_the_document() {
    let items = sample_items();

    assert_eq!(
        item(&items, "Insurance/2023/2024-05-01_invoice.pdf").date,
        day(2024, 5, 1)
    );
    assert_eq!(
        item(&items, "Insurance/2024/2024-07-01_invoice.pdf").date,
        None
    );
}

#[test]
fn a_long_condition_is_cut_for_the_label() {
    let spec = rule_spec(&format!("  {}  ", "x".repeat(60)), "Tax", "", true);

    assert_eq!(
        rule_label(&spec),
        format!("{}…", "x".repeat(RULE_LABEL_CHARS))
    );
}

#[test]
fn a_rule_without_a_usable_condition_is_labelled_by_its_kind() {
    let mut spec = rule_spec("  ", "Tax", "", true);
    spec.conditions.list.push(Condition {
        negated: true,
        ..Condition::new(ConditionKind::Substring, "other")
    });

    assert_eq!(rule_label(&spec), "Tax");
}

#[test]
fn making_the_plan_changes_no_file() {
    let archive = archive();
    let before = archive.snapshot();

    let items = archive.access().plan().unwrap();

    assert!(!items.is_empty());
    assert_eq!(archive.snapshot(), before);
}

fn moved_target(entry: &PlannedItem) -> &Path {
    match &entry.item.outcome {
        PlanOutcome::Move { target } | PlanOutcome::Rename { target } => target,
        other => panic!("expected a move, got {other:?}"),
    }
}

#[test]
fn executing_a_plan_entry_moves_the_file_and_repoints_the_index() {
    let archive = archive();
    let access = archive.access();
    let items = access.plan().unwrap();
    let entry = item(&items, "Insurance/2023/2024-05-01_invoice.pdf");
    let source = archive.root.join(&entry.item.source);
    let target = archive.root.join(moved_target(entry));
    let content = fs::read(&source).unwrap();
    let id_before = archive
        .slot
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .document_by_path(&source)
        .unwrap()
        .unwrap()
        .id;

    let summary = access
        .execute(
            std::slice::from_ref(entry),
            &mut ncrs_fs::transfer::Counting,
        )
        .unwrap();

    assert_eq!(summary.moved, 1);
    assert_eq!(fs::read(&target).unwrap(), content);
    assert!(!source.exists());
    let guard = archive.slot.lock().unwrap();
    let index = guard.as_ref().unwrap();
    assert_eq!(
        index.document_by_path(&target).unwrap().map(|d| d.id),
        Some(id_before)
    );
    assert!(index.document_by_path(&source).unwrap().is_none());
}

#[test]
fn undoing_an_executed_plan_entry_puts_the_file_back() {
    let archive = archive();
    let access = archive.access();
    let items = access.plan().unwrap();
    let entries = [
        item(&items, "Insurance/2023/2024-05-01_invoice.pdf").clone(),
        item(&items, "Misc/2024-04-01_invoice.pdf").clone(),
    ];
    let before = archive.snapshot();

    let summary = access
        .execute(&entries, &mut ncrs_fs::transfer::Counting)
        .unwrap();
    assert_eq!(summary.moved, 2);
    assert_ne!(archive.snapshot(), before);

    let report = access.undo_last().unwrap().unwrap();

    assert_eq!(report.undone, 2);
    assert_eq!(archive.snapshot(), before);
}

#[test]
fn a_closed_index_is_reported() {
    let access = PlanAccess::new(Arc::new(Mutex::new(None)), vec![PathBuf::from("/x")]);

    assert_eq!(access.plan(), Err(INDEX_NOT_OPEN.to_owned()));
}

#[test]
fn executing_and_undoing_with_a_closed_index_is_reported() {
    let access = PlanAccess::new(Arc::new(Mutex::new(None)), vec![PathBuf::from("/x")]);

    assert_eq!(
        access.execute(&[], &mut ncrs_fs::transfer::Counting),
        Err(INDEX_NOT_OPEN.to_owned())
    );
    assert_eq!(access.undo_last(), Err(INDEX_NOT_OPEN.to_owned()));
}
