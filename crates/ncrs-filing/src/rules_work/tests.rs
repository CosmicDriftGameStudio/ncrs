use std::collections::BTreeSet;

use ncrs_index::{
    Condition, ConditionKind, ContentHash, RuleConditions, RuleEvidence, RuleOrigin, RuleSpec,
    RuleSuggestion, TargetId,
};
use tempfile::TempDir;

use super::*;

fn spec(word: &str) -> RuleSpec {
    RuleSpec {
        name: None,
        conditions: RuleConditions::all_of(vec![Condition::new(ConditionKind::Word, word)]),
        extract_number: None,
        extract_date: None,
        extract_amount: None,
        kind: Some("invoice".to_owned()),
        tags: BTreeSet::new(),
        filing_pattern: "{year}".to_owned(),
        confirmed: true,
        enabled: true,
    }
}

struct Fixture {
    _dir: TempDir,
    access: RulesAccess,
    slot: IndexSlot,
}

fn fixture() -> Fixture {
    let dir = TempDir::new().unwrap();
    let index = Index::create_plain(&dir.path().join("index.db")).unwrap();
    let slot: IndexSlot = Arc::new(Mutex::new(Some(index)));
    Fixture {
        access: RulesAccess::new(Arc::clone(&slot)),
        _dir: dir,
        slot,
    }
}

impl Fixture {
    fn with_index<T>(&self, work: impl FnOnce(&mut Index) -> T) -> T {
        work(self.slot.lock().unwrap().as_mut().unwrap())
    }
}

/// Rules a, suggestion s, rule b, in that order.
fn a_suggestion_between_two_rules(fixture: &Fixture) -> [RuleId; 3] {
    fixture.with_index(|index| {
        let a = index
            .create_rule_in(&spec("rent"), RuleOrigin::Filing, &TargetId::main())
            .unwrap();
        let RuleSuggestion::Created(s) = index
            .suggest_rule_in(
                &spec("tax"),
                RuleEvidence {
                    matching_in_folder: 3,
                    folder_documents: 3,
                    matching_elsewhere: 0,
                    subtree: None,
                },
                &TargetId::main(),
            )
            .unwrap()
        else {
            panic!("not created");
        };
        let b = index.create_rule(&spec("power")).unwrap();
        [a, s, b]
    })
}

fn listed_ids(fixture: &Fixture) -> Vec<RuleId> {
    fixture
        .access
        .load()
        .unwrap()
        .rules
        .iter()
        .map(|listed| listed.rule.id)
        .collect()
}

#[test]
fn the_load_lists_only_confirmed_rules_with_their_place_among_all() {
    let fixture = fixture();
    let [a, _s, b] = a_suggestion_between_two_rules(&fixture);

    let load = fixture.access.load().unwrap();

    let placed: Vec<_> = load
        .rules
        .iter()
        .map(|listed| (listed.rule.id, listed.global_index, listed.hits))
        .collect();
    assert_eq!(placed, [(a, 0, 0), (b, 2, 0)]);
}

#[test]
fn moving_down_over_a_suggestion_lands_after_the_next_confirmed_rule() {
    let fixture = fixture();
    let [a, s, b] = a_suggestion_between_two_rules(&fixture);
    let load = fixture.access.load().unwrap();
    let next = load.rules[1].global_index;

    fixture
        .access
        .apply(RuleChange::Move {
            id: a,
            to_index: next,
        })
        .unwrap();

    assert_eq!(listed_ids(&fixture), [b, a]);
    let all: Vec<_> =
        fixture.with_index(|index| index.rules().unwrap().iter().map(|rule| rule.id).collect());
    assert_eq!(all, [s, b, a]);
}

#[test]
fn moving_up_over_a_suggestion_lands_before_the_previous_confirmed_rule() {
    let fixture = fixture();
    let [a, _s, b] = a_suggestion_between_two_rules(&fixture);
    let previous = fixture
        .access
        .load()
        .unwrap()
        .rules
        .first()
        .unwrap()
        .global_index;

    fixture
        .access
        .apply(RuleChange::Move {
            id: b,
            to_index: previous,
        })
        .unwrap();

    assert_eq!(listed_ids(&fixture), [b, a]);
}

#[test]
fn switching_a_rule_off_and_on_is_kept() {
    let fixture = fixture();
    let [a, _s, _b] = a_suggestion_between_two_rules(&fixture);

    fixture
        .access
        .apply(RuleChange::SetEnabled {
            id: a,
            enabled: false,
        })
        .unwrap();

    let enabled_of = |of: &Fixture| of.access.load().unwrap().rules[0].rule.spec.enabled;
    assert!(!enabled_of(&fixture));
    fixture
        .access
        .apply(RuleChange::SetEnabled {
            id: a,
            enabled: true,
        })
        .unwrap();
    assert!(enabled_of(&fixture));
}

#[test]
fn deleting_a_rule_removes_it_from_the_list() {
    let fixture = fixture();
    let [a, _s, b] = a_suggestion_between_two_rules(&fixture);

    fixture.access.apply(RuleChange::Delete { id: a }).unwrap();

    assert_eq!(listed_ids(&fixture), [b]);
}

#[test]
fn a_created_rule_is_listed_with_the_origin_hand() {
    let fixture = fixture();

    fixture
        .access
        .save(RuleSave::Create {
            spec: spec("rent"),
            origin: RuleOrigin::Hand,
            target: TargetId::main(),
        })
        .unwrap();

    let load = fixture.access.load().unwrap();
    assert_eq!(load.rules.len(), 1);
    assert_eq!(load.rules[0].rule.origin, RuleOrigin::Hand);
    assert_eq!(load.rules[0].rule.spec, spec("rent"));
}

#[test]
fn a_rule_made_from_a_filed_document_has_the_origin_filing() {
    let fixture = fixture();

    fixture
        .access
        .save(RuleSave::Create {
            spec: spec("rent"),
            origin: RuleOrigin::Filing,
            target: TargetId::main(),
        })
        .unwrap();

    let load = fixture.access.load().unwrap();
    assert_eq!(load.rules[0].rule.origin, RuleOrigin::Filing);
}

#[test]
fn confirming_a_suggestion_makes_it_a_listed_rule_and_keeps_its_origin() {
    let fixture = fixture();
    let [_a, s, _b] = a_suggestion_between_two_rules(&fixture);
    let mut edited = spec("tax");
    edited.filing_pattern = "{year}/{sender}".to_owned();

    fixture
        .access
        .save(RuleSave::ConfirmSuggestion(s, edited.clone(), None))
        .unwrap();

    let load = fixture.access.load().unwrap();
    let confirmed = load
        .rules
        .iter()
        .find(|listed| listed.rule.id == s)
        .unwrap();
    assert!(confirmed.rule.spec.confirmed);
    assert_eq!(confirmed.rule.spec.filing_pattern, "{year}/{sender}");
    assert_eq!(confirmed.rule.origin, RuleOrigin::Suggestion);
    assert_eq!(load.rules.len(), 3);
}

#[test]
fn confirming_a_suggestion_that_is_gone_fails_with_a_plain_message() {
    let fixture = fixture();
    let [_a, s, _b] = a_suggestion_between_two_rules(&fixture);
    fixture
        .with_index(|index| index.discard_rule_suggestion(s))
        .unwrap();

    let error = fixture
        .access
        .save(RuleSave::ConfirmSuggestion(s, spec("tax"), None))
        .unwrap_err();

    assert_eq!(error, NO_SUCH_SUGGESTION);
}

#[test]
fn an_update_changes_the_form_fields_and_keeps_the_rest_of_the_rule() {
    let fixture = fixture();
    let id = fixture.with_index(|index| {
        let mut stored = spec("rent");
        stored.extract_date = Some(r"\d{4}".to_owned());
        stored.enabled = false;
        let id = index
            .create_rule_in(&stored, RuleOrigin::Filing, &TargetId::main())
            .unwrap();
        index.create_rule(&spec("power")).unwrap();
        id
    });
    let before = fixture.access.load().unwrap().rules[0].clone();
    let changed = RuleSpec {
        conditions: RuleConditions::any_of(vec![Condition::new(ConditionKind::Sender, "Telekom")]),
        filing_pattern: "{year}/{sender}".to_owned(),
        ..before.rule.spec.clone()
    };

    fixture
        .access
        .save(RuleSave::Update(id, changed.clone(), None))
        .unwrap();

    let after = fixture.access.load().unwrap().rules[0].clone();
    assert_eq!(after.rule.spec, changed);
    assert_eq!(after.rule.spec.extract_date.as_deref(), Some(r"\d{4}"));
    assert!(!after.rule.spec.enabled);
    assert_eq!(after.rule.origin, RuleOrigin::Filing);
    assert_eq!(after.rule.position, before.rule.position);
}

#[test]
fn a_regex_the_index_refuses_comes_back_as_the_error_text() {
    let fixture = fixture();
    let mut broken = spec("rent");
    broken.conditions = RuleConditions::all_of(vec![Condition::new(ConditionKind::Regex, "(")]);

    let error = fixture
        .access
        .save(RuleSave::Create {
            spec: broken,
            origin: RuleOrigin::Hand,
            target: TargetId::main(),
        })
        .unwrap_err();

    assert!(!error.is_empty());
    assert!(fixture.access.load().unwrap().rules.is_empty());
}

#[test]
fn without_an_index_the_work_fails_with_a_plain_message() {
    let access = RulesAccess::new(Arc::new(Mutex::new(None)));

    assert_eq!(access.load().unwrap_err(), INDEX_NOT_OPEN);
}

fn archive_move(from: &str, to: &str) -> ArchiveMove {
    ArchiveMove {
        root: PathBuf::from("/archive"),
        from: PathBuf::from(from),
        to: PathBuf::from(to),
        hash: ContentHash([0; 32]),
    }
}

#[test]
fn a_chain_of_moves_runs_from_its_end_and_the_name_two_want_goes_to_the_first() {
    let (ordered, blocked) = order_moves(vec![
        archive_move("a.pdf", "b.pdf"),
        archive_move("b.pdf", "c.pdf"),
        archive_move("x.pdf", "c.pdf"),
    ]);

    let steps: Vec<_> = ordered
        .iter()
        .map(|found| (found.from.to_str().unwrap(), found.to.to_str().unwrap()))
        .collect();
    assert_eq!(steps, vec![("b.pdf", "c.pdf"), ("a.pdf", "b.pdf")]);
    assert_eq!(blocked, 1);
}

#[test]
fn a_swap_of_two_names_cannot_run_and_is_counted() {
    let (ordered, blocked) = order_moves(vec![
        archive_move("a.pdf", "b.pdf"),
        archive_move("b.pdf", "a.pdf"),
        archive_move("c.pdf", "d.pdf"),
    ]);

    assert_eq!(ordered, vec![archive_move("c.pdf", "d.pdf")]);
    assert_eq!(blocked, 2);
}

#[test]
fn a_saved_rule_goes_to_its_target_and_can_move_to_another() {
    let fixture = fixture();
    let other = TargetId::new("t2").unwrap();
    let id = fixture
        .access
        .save(RuleSave::Create {
            spec: spec("rent"),
            origin: RuleOrigin::Hand,
            target: other.clone(),
        })
        .unwrap();
    assert_eq!(fixture.access.load().unwrap().rules[0].rule.target, other);

    fixture
        .access
        .save(RuleSave::Update(id, spec("rent"), Some(TargetId::main())))
        .unwrap();

    assert_eq!(
        fixture.access.load().unwrap().rules[0].rule.target,
        TargetId::main()
    );
}

#[test]
fn removing_a_target_deletes_its_rules_only() {
    let fixture = fixture();
    let other = TargetId::new("t2").unwrap();
    for target in [TargetId::main(), other.clone()] {
        fixture
            .access
            .save(RuleSave::Create {
                spec: spec("rent"),
                origin: RuleOrigin::Hand,
                target,
            })
            .unwrap();
    }

    assert_eq!(fixture.access.delete_rules_of(&other).unwrap(), 1);

    let left = fixture.access.load().unwrap().rules;
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].rule.target, TargetId::main());
}
