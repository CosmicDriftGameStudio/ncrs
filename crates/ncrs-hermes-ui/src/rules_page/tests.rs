use super::fixtures::{listed, page, rule_id, spec};
use super::*;
use ncrs_index::{Index, RuleConditions};
use std::collections::BTreeSet;

#[test]
fn the_rule_to_select_on_load_is_the_selected_one() {
    let mut page = RulesPage::new();
    page.select_on_load(rule_id(3));

    page.loaded(RulesLoad {
        rules: vec![listed(1, 0), listed(2, 2), listed(3, 3)],
    });

    assert_eq!(page.selected().map(|row| row.rule.id), Some(rule_id(3)));
}

#[test]
fn a_rule_to_select_that_is_not_listed_keeps_the_cursor() {
    let mut page = RulesPage::new();
    page.select_on_load(rule_id(9));

    page.loaded(RulesLoad {
        rules: vec![listed(1, 0), listed(2, 2)],
    });

    assert_eq!(page.selected().map(|row| row.rule.id), Some(rule_id(1)));
}

#[test]
fn a_form_opened_from_elsewhere_survives_the_load() {
    let mut page = RulesPage::new();
    page.open_form(RuleForm::new());

    page.loaded(RulesLoad::default());

    assert!(page.form().is_some());
    assert!(page.dialog_is_open());
}

#[test]
fn a_new_page_is_busy_until_the_list_is_loaded() {
    let mut page = RulesPage::new();
    assert!(page.is_busy());

    page.loaded(RulesLoad::default());

    assert!(!page.is_busy());
    assert!(page.selected().is_none());
}

#[test]
fn a_reload_keeps_the_cursor_on_the_same_rule() {
    let mut page = page();
    page.begin_edit(2);

    page.loaded(RulesLoad {
        rules: vec![listed(3, 0), listed(1, 1), listed(2, 2)],
    });

    assert_eq!(page.selected().unwrap().rule.id, rule_id(3));
}

#[test]
fn a_toggle_asks_for_the_opposite_of_the_current_switch() {
    let mut page = page();

    let change = page.begin_toggle(1);

    assert_eq!(
        change,
        Some(RuleChange::SetEnabled {
            id: rule_id(2),
            enabled: false
        })
    );
    assert!(page.is_busy());
    assert_eq!(page.begin_toggle(0), None);
}

#[test]
fn moving_down_targets_the_place_of_the_next_listed_rule_among_all() {
    let mut page = page();

    let change = page.begin_move(0, 1);

    assert_eq!(
        change,
        Some(RuleChange::Move {
            id: rule_id(1),
            to_index: 2
        })
    );
}

#[test]
fn moving_up_targets_the_place_of_the_previous_listed_rule_among_all() {
    let mut page = page();

    let change = page.begin_move(1, -1);

    assert_eq!(
        change,
        Some(RuleChange::Move {
            id: rule_id(2),
            to_index: 0
        })
    );
}

#[test]
fn the_first_rule_cannot_move_up_and_the_last_cannot_move_down() {
    let mut page = page();
    assert_eq!(page.begin_move(0, -1), None);
    assert!(!page.is_busy());

    assert_eq!(page.begin_move(2, 1), None);
    assert!(!page.is_busy());
}

#[test]
fn deleting_needs_the_question_first() {
    let mut page = page();
    assert_eq!(page.begin_delete(), None);

    page.ask_delete(0);
    assert!(page.is_confirming_delete());
    assert!(page.dialog_is_open());
    let change = page.begin_delete();

    assert_eq!(change, Some(RuleChange::Delete { id: rule_id(1) }));
}

#[test]
fn the_question_is_about_the_rule_it_was_asked_on() {
    let mut page = page();

    page.ask_delete(2);

    assert_eq!(
        page.begin_delete(),
        Some(RuleChange::Delete { id: rule_id(3) })
    );
}

#[test]
fn a_change_under_way_withdraws_the_delete_question() {
    let mut page = page();
    page.ask_delete(0);

    assert!(page.begin_toggle(0).is_some());

    assert!(!page.is_confirming_delete());
    assert_eq!(page.begin_delete(), None);
}

#[test]
fn cancelling_the_question_deletes_nothing() {
    let mut page = page();
    page.ask_delete(0);

    assert!(page.cancel_delete());

    assert!(!page.is_confirming_delete());
    assert_eq!(page.begin_delete(), None);
    assert!(!page.cancel_delete());
}

#[test]
fn cancelling_a_form_closes_the_dialog_and_drops_what_was_typed() {
    let mut page = page();
    page.begin_new();
    assert!(page.dialog_is_open());
    page.form_mut()
        .unwrap()
        .update(crate::message::RuleFormMessage::Name("Contract".to_owned()));
    assert!(page.form().unwrap().is_changed());

    assert!(page.cancel_form());

    assert!(page.form().is_none());
    assert!(!page.dialog_is_open());
    assert!(!page.cancel_form());
}

#[test]
fn editing_a_rule_opens_the_form_of_that_rule() {
    let mut page = page();

    page.begin_edit(1);

    assert!(matches!(
        page.form().unwrap().target,
        crate::rule_form::RuleFormTarget::Edit(_)
    ));
    assert_eq!(page.selected().unwrap().rule.id, rule_id(2));
}

#[test]
fn while_a_change_runs_the_list_takes_no_change() {
    let mut page = page();
    assert!(page.begin_toggle(0).is_some());

    page.ask_delete(2);
    page.begin_new();
    page.begin_edit(1);

    assert!(!page.is_confirming_delete());
    assert!(page.form().is_none());
    assert_eq!(page.begin_toggle(0), None);
    assert_eq!(page.begin_move(0, 1), None);
}

#[test]
fn saving_a_new_form_creates_the_rule_and_an_edit_form_changes_it() {
    use crate::condition_editor::ConditionEdit;
    use crate::filing_pieces::TargetEdit;
    use crate::message::RuleFormMessage;
    use ncrs_filing::rules_work::RulesAccess;
    use std::sync::{Arc, Mutex};

    let dir = tempfile::TempDir::new().unwrap();
    let index = Index::create_plain(&dir.path().join("index.db")).unwrap();
    let access = RulesAccess::new(Arc::new(Mutex::new(Some(index))), Vec::new());
    let mut page = RulesPage::new();
    page.loaded(access.load().unwrap());

    page.begin_new();
    assert_eq!(page.begin_save(), None);
    page.form_mut()
        .unwrap()
        .update(RuleFormMessage::Condition(ConditionEdit::Value(
            0,
            "telekom".to_owned(),
        )));
    let created = access.save(page.begin_save().unwrap()).unwrap();
    page.saved(created);
    page.loaded(access.load().unwrap());

    assert!(page.form().is_none());
    let sentence = rule_sentence(
        &page.selected().unwrap().rule.spec,
        Language::English,
        &KindCatalog::built_in(),
    );
    assert_eq!(sentence, "If telekom → Year / Sender");

    page.begin_edit(0);
    page.form_mut()
        .unwrap()
        .update(RuleFormMessage::Target(TargetEdit::Remove(1)));
    let changed = access.save(page.begin_save().unwrap()).unwrap();
    page.saved(changed);
    page.loaded(access.load().unwrap());

    assert_eq!(page.rows().len(), 1);
    assert_eq!(page.selected().unwrap().rule.spec.filing_pattern, "{year}/");
}

#[test]
fn a_failed_save_keeps_the_form_with_the_reason() {
    let mut page = page();
    page.begin_edit(0);
    page.begin_save().unwrap();

    page.save_failed("index not open".to_owned());

    assert_eq!(
        page.form().unwrap().failure.as_deref(),
        Some("index not open")
    );
    assert!(page.begin_save().is_some());
}

#[test]
fn a_failure_is_shown_and_frees_the_page() {
    let mut page = page();
    page.begin_toggle(0);

    page.failed("index not open".to_owned());

    assert!(!page.is_busy());
    assert_eq!(page.problem(), Some("index not open"));
}

#[test]
fn a_sentence_joins_all_of_with_and_and_ends_in_the_target() {
    let plain = spec(RuleConditions::all_of(vec![
        Condition::new(ConditionKind::Word, "rent"),
        Condition::new(ConditionKind::Sender, "Company A"),
    ]));
    let spec = RuleSpec {
        filing_pattern: "{year}/{sender}/".to_owned(),
        ..plain
    };

    assert_eq!(
        rule_sentence(&spec, Language::English, &KindCatalog::built_in()),
        "If the text contains the word “rent” and the sender is “Company A” → \
         Invoice · “office” “tax” · Year / Sender"
    );
}

#[test]
fn a_sentence_joins_any_of_with_or_and_words_a_negation() {
    let spec = RuleSpec {
        tags: BTreeSet::new(),
        filing_pattern: "{year}/{sender}/".to_owned(),
        ..spec(RuleConditions::any_of(vec![
            Condition::not(ConditionKind::Sender, "Agency B"),
            Condition::new(ConditionKind::Regex, r"No\. \d+"),
        ]))
    };

    assert_eq!(
        rule_sentence(&spec, Language::English, &KindCatalog::built_in()),
        "If the sender is not “Agency B” or the text matches /No\\. \\d+/ → \
         Invoice · Year / Sender"
    );
}

#[test]
fn a_german_sentence_uses_german_quotes_and_joiners() {
    let spec = RuleSpec {
        tags: BTreeSet::new(),
        filing_pattern: "{year}/{sender}/".to_owned(),
        ..spec(RuleConditions::any_of(vec![
            Condition::not(ConditionKind::Word, "Mahnung"),
            Condition::new(ConditionKind::Folder, "Post/2024"),
        ]))
    };

    assert_eq!(
        rule_sentence(&spec, Language::German, &KindCatalog::built_in()),
        "Wenn der Text das Wort „Mahnung“ nicht enthält oder das Dokument in \
         „Post/2024“ liegt → Rechnung · Jahr / Absender"
    );
}

#[test]
fn every_kind_has_a_sentence_in_both_languages_with_and_without_negation() {
    for lang in [Language::English, Language::German] {
        for kind in ConditionKind::ALL {
            for negated in [false, true] {
                let condition = Condition {
                    kind,
                    value: "VALUE".to_owned(),
                    negated,
                };
                let sentence = condition_sentence(&condition, lang, &KindCatalog::built_in());
                assert!(sentence.contains("VALUE"), "{kind:?} {negated}: {sentence}");
                assert!(!sentence.contains("{value}"), "{sentence}");
            }
        }
    }
}

#[test]
fn hits_are_counted_in_singular_and_plural() {
    assert_eq!(hits_text(1, Language::English), "1 hit");
    assert_eq!(hits_text(0, Language::English), "0 hits");
    assert_eq!(hits_text(12, Language::German), "12 Treffer");
}
