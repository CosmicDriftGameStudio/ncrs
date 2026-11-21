use std::collections::BTreeSet;

use ncrs_index::{Condition, ConditionKind, RuleConditions, RuleSaved, RuleSpec};

use super::*;

fn named_rule(name: &str) -> RuleSpec {
    RuleSpec {
        name: Some(name.to_owned()),
        conditions: RuleConditions::all_of(vec![Condition::new(ConditionKind::Substring, "acme")]),
        extract_number: None,
        extract_date: None,
        extract_amount: None,
        kind: Some("invoice".to_owned()),
        tags: BTreeSet::new(),
        filing_pattern: "Acme/{kind}.pdf".to_owned(),
        confirmed: true,
        enabled: true,
    }
}

fn while_a_run_holds_the_index<T>(setup: &Setup, ask: impl FnOnce(&ChatAccess) -> T) -> T {
    let (locked_tx, locked_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let slot = Arc::clone(&setup.access.slot);
    let holder = std::thread::spawn(move || {
        let _guard = slot.lock().unwrap();
        locked_tx.send(()).unwrap();
        let _ = release_rx.recv();
    });
    locked_rx.recv().unwrap();
    let answer = ask(&setup.access);
    release_tx.send(()).unwrap();
    holder.join().unwrap();
    answer
}

fn names(rules: &[Rule]) -> Vec<Option<&str>> {
    rules.iter().map(|rule| rule.spec.name.as_deref()).collect()
}

#[test]
fn the_ui_still_sees_a_rule_saved_from_the_chat_while_a_run_holds_the_index() {
    let setup = Setup::new();
    assert!(setup.access.try_rules().unwrap().is_empty());
    let saved = setup.access.save_rule(&named_rule("Acme invoices"));
    assert_eq!(saved.ok(), Some(RuleSaved::Added));

    let busy = while_a_run_holds_the_index(&setup, ChatAccess::try_rules);

    assert_eq!(names(&busy.unwrap()), vec![Some("Acme invoices")]);
}

#[test]
fn saving_a_rule_whose_name_exists_changes_it_instead_of_adding_a_second_one() {
    let setup = Setup::new();
    assert_eq!(
        setup.access.save_rule(&named_rule("Acme invoices")).ok(),
        Some(RuleSaved::Added)
    );

    let again = setup.access.save_rule(&named_rule("Acme invoices"));

    assert_eq!(again.ok(), Some(RuleSaved::Changed));
    assert_eq!(setup.access.try_rules().unwrap().len(), 1);
}

#[test]
fn rules_read_while_the_index_was_free_are_what_the_ui_gets_while_it_is_busy() {
    let setup = Setup::new();
    setup
        .access
        .with_index_mut(|index| {
            index
                .create_rule(&named_rule("Acme invoices"))
                .map(|_| ())
                .map_err(|err| err.to_string())
        })
        .unwrap();
    let known = setup.access.try_rules().unwrap();

    let busy = while_a_run_holds_the_index(&setup, ChatAccess::try_rules);

    assert_eq!(busy.map(|rules| names(&rules).len()), Ok(known.len()));
}

#[test]
fn a_rule_deleted_on_the_rules_page_is_not_shown_from_the_snapshot_while_the_index_is_busy() {
    let setup = Setup::new();
    setup
        .access
        .save_rule(&named_rule("Acme invoices"))
        .unwrap();
    let shown = setup.access.try_rules().unwrap();
    let page = crate::rules_work::RulesAccess::new(Arc::clone(&setup.access.slot), Vec::new())
        .with_snapshots(setup.access.snapshots.clone());

    page.apply(crate::rules_work::RuleChange::Delete { id: shown[0].id })
        .unwrap();
    let busy = while_a_run_holds_the_index(&setup, ChatAccess::try_rules);

    assert!(busy.is_err(), "{busy:?}");
}
