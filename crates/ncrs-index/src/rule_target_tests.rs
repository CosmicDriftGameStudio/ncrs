use crate::model::{
    Condition, ConditionKind, RuleConditions, RuleEvidence, RuleId, RuleOrigin, RuleSpec,
    RuleSuggestion, TargetId,
};
use crate::test_support::{encrypted_index, rule_spec};
use crate::Index;

fn target(name: &str) -> TargetId {
    TargetId::new(name).unwrap()
}

fn ids_of(index: &Index, target: &TargetId) -> Vec<RuleId> {
    index
        .rules_of(target)
        .unwrap()
        .iter()
        .map(|rule| rule.id)
        .collect()
}

fn positions(index: &Index) -> Vec<(RuleId, i64)> {
    index
        .rules()
        .unwrap()
        .iter()
        .map(|rule| (rule.id, rule.position))
        .collect()
}

fn add(index: &mut Index, name: &str, target: &TargetId) -> RuleId {
    let spec = RuleSpec {
        name: Some(name.to_owned()),
        ..rule_spec()
    };
    index
        .create_rule_in(&spec, RuleOrigin::Hand, target)
        .unwrap()
}

fn evidence() -> RuleEvidence {
    RuleEvidence {
        matching_in_folder: 5,
        folder_documents: 5,
        matching_elsewhere: 0,
        subtree: None,
    }
}

#[test]
fn a_rule_moves_only_among_the_rules_of_its_target_and_the_others_keep_their_positions() {
    let (_dir, mut index) = encrypted_index();
    let (first, second) = (target("t1"), target("t2"));
    let a1 = add(&mut index, "a1", &first);
    let b1 = add(&mut index, "b1", &second);
    let a2 = add(&mut index, "a2", &first);
    let b2 = add(&mut index, "b2", &second);
    let a3 = add(&mut index, "a3", &first);
    assert_eq!(
        positions(&index),
        [(a1, 1), (b1, 2), (a2, 3), (b2, 4), (a3, 5)]
    );

    index.move_rule(a3, 0).unwrap();

    assert_eq!(ids_of(&index, &first), [a3, a1, a2]);
    assert_eq!(ids_of(&index, &second), [b1, b2]);
    assert_eq!(
        positions(&index),
        [(a3, 1), (b1, 2), (a1, 3), (b2, 4), (a2, 5)]
    );
}

#[test]
fn a_rule_listed_for_a_target_carries_it_and_the_listing_of_all_has_every_target() {
    let (_dir, mut index) = encrypted_index();
    let second = target("t2");
    let main = add(&mut index, "m", &TargetId::main());
    let other = add(&mut index, "o", &second);

    assert_eq!(index.rule(other).unwrap().unwrap().target, second);
    assert_eq!(ids_of(&index, &TargetId::main()), [main]);
    assert_eq!(index.rules().unwrap().len(), 2);
    assert!(index.rules_of(&target("t3")).unwrap().is_empty());
}

#[test]
fn deleting_the_rules_of_a_target_takes_only_its_own() {
    let (_dir, mut index) = encrypted_index();
    let (first, second) = (target("t1"), target("t2"));
    add(&mut index, "a", &first);
    let b = add(&mut index, "b", &second);
    add(&mut index, "c", &first);

    assert_eq!(index.delete_rules_of(&first).unwrap(), 2);

    assert!(index.rules_of(&first).unwrap().is_empty());
    assert_eq!(ids_of(&index, &second), [b]);
    assert_eq!(index.delete_rules_of(&first).unwrap(), 0);
}

#[test]
fn a_suggestion_discarded_for_one_target_is_still_offered_in_another() {
    let (_dir, mut index) = encrypted_index();
    let (first, second) = (target("t1"), target("t2"));
    let spec = RuleSpec {
        conditions: RuleConditions::all_of(vec![Condition::new(
            ConditionKind::Substring,
            "company a ltd",
        )]),
        confirmed: true,
        ..rule_spec()
    };
    let RuleSuggestion::Created(id) = index.suggest_rule_in(&spec, evidence(), &first).unwrap()
    else {
        panic!("not created");
    };
    assert_eq!(index.rule(id).unwrap().unwrap().target, first);
    assert!(index.discard_rule_suggestion(id).unwrap());

    assert_eq!(
        index.suggest_rule_in(&spec, evidence(), &first).unwrap(),
        RuleSuggestion::Discarded
    );
    let RuleSuggestion::Created(other) = index.suggest_rule_in(&spec, evidence(), &second).unwrap()
    else {
        panic!("not offered for the second target");
    };
    assert_eq!(index.rule(other).unwrap().unwrap().target, second);
    assert_eq!(
        index.suggest_rule_in(&spec, evidence(), &second).unwrap(),
        RuleSuggestion::Refreshed(other)
    );
}

#[test]
fn a_rule_with_a_target_name_the_index_cannot_read_is_an_error_and_not_main() {
    let (_dir, mut index) = encrypted_index();
    let id = add(&mut index, "x", &TargetId::main());
    index
        .connection()
        .execute(
            "UPDATE rule SET target = 'Bad Name' WHERE id = ?1",
            [id.get()],
        )
        .unwrap();

    let error = index.rule(id).unwrap_err();

    assert!(error.to_string().contains("Bad Name"), "{error}");
    assert!(index.rules_of(&TargetId::main()).unwrap().is_empty());
}
