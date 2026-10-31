//! The work behind the rules page: reading the confirmed rules with their hits,
//! switching, moving and deleting one. It blocks, so `App` runs it on a
//! blocking thread, never in `update`.

use std::sync::{Arc, Mutex, PoisonError};

use ncrs_index::{Index, IndexError, Rule, RuleId, TaxContextEntry};

use crate::suggestion_work::INDEX_NOT_OPEN;

type IndexSlot = Arc<Mutex<Option<Index>>>;

/// A confirmed rule as the page lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedRule {
    pub rule: Rule,
    /// Where the rule stands among all rules, suggestions included: the
    /// index orders them together.
    pub global_index: usize,
    pub hits: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RulesLoad {
    pub rules: Vec<ListedRule>,
    pub tax_contexts: Vec<TaxContextEntry>,
}

/// What the user asked to change about one rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleChange {
    SetEnabled {
        id: RuleId,
        enabled: bool,
    },
    /// `to_index` counts in the order of all rules.
    Move {
        id: RuleId,
        to_index: usize,
    },
    Delete {
        id: RuleId,
    },
}

#[derive(Clone)]
pub struct RulesAccess {
    slot: IndexSlot,
}

impl RulesAccess {
    pub fn new(slot: IndexSlot) -> Self {
        Self { slot }
    }

    pub fn load(&self) -> Result<RulesLoad, String> {
        self.with_index(|index| {
            let hits = index.rule_hits()?;
            let rules = index
                .rules()?
                .into_iter()
                .enumerate()
                .filter(|(_, rule)| rule.spec.confirmed)
                .map(|(global_index, rule)| ListedRule {
                    hits: hits.get(&rule.id).copied().unwrap_or(0),
                    global_index,
                    rule,
                })
                .collect();
            Ok(RulesLoad {
                rules,
                tax_contexts: index.tax_contexts()?,
            })
        })
    }

    pub fn apply(&self, change: RuleChange) -> Result<(), String> {
        self.with_index(|index| match change {
            RuleChange::SetEnabled { id, enabled } => {
                index.set_rule_enabled(id, enabled).map(|_| ())
            }
            RuleChange::Move { id, to_index } => index.move_rule(id, to_index),
            RuleChange::Delete { id } => index.delete_rule(id).map(|_| ()),
        })
    }

    fn with_index<T>(
        &self,
        work: impl FnOnce(&mut Index) -> Result<T, IndexError>,
    ) -> Result<T, String> {
        let mut guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let index = guard.as_mut().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        work(index).map_err(|err| err.to_string())
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use std::collections::BTreeSet;

    use ncrs_index::{
        Condition, ConditionKind, RuleConditions, RuleEvidence, RuleOrigin, RuleSpec,
        RuleSuggestion, TaxContext,
    };
    use tempfile::TempDir;

    use super::*;

    fn spec(word: &str) -> RuleSpec {
        RuleSpec {
            conditions: RuleConditions::all_of(vec![Condition::new(ConditionKind::Word, word)]),
            extract_number: None,
            extract_date: None,
            extract_amount: None,
            kind: "Invoice".to_owned(),
            tax_context: Some(TaxContext::None),
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
            _dir: dir,
            access: RulesAccess::new(Arc::clone(&slot)),
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
                .create_rule_from(&spec("rent"), RuleOrigin::Filing)
                .unwrap();
            let RuleSuggestion::Created(s) = index
                .suggest_rule(
                    &spec("tax"),
                    RuleEvidence {
                        matching_in_folder: 3,
                        folder_documents: 3,
                        matching_elsewhere: 0,
                        subtree: None,
                    },
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
    fn without_an_index_the_work_fails_with_a_plain_message() {
        let access = RulesAccess::new(Arc::new(Mutex::new(None)));

        assert_eq!(access.load().unwrap_err(), INDEX_NOT_OPEN);
    }
}
