//! The work behind the rules page: reading the confirmed rules with their hits,
//! switching, moving and deleting one. It blocks, so `App` runs it on a
//! blocking thread, never in `update`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use ncrs_index::{Index, IndexError, ReviewReason, Rule, RuleId, RuleOrigin, RuleSpec};
use ncrs_intake::{plan, Location, LocationMode, PlanInput, Planning};

use crate::rule_form::RuleSave;
use crate::suggestion_work::INDEX_NOT_OPEN;

/// Another confirm or a discard got to the suggestion first.
const NO_SUCH_SUGGESTION: &str = "the suggested rule is not there any more";

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

/// The rule the form describes, to try against the documents.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleTry {
    /// The stored rule the candidate takes the place of; `None` for a rule
    /// that is appended.
    pub replaces: Option<RuleId>,
    pub spec: RuleSpec,
}

/// Where a document is now. The order is the order of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrySection {
    Inbox,
    Review,
    Archive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TryOutcome {
    /// Would be filed here, relative to the archive.
    Filed(PathBuf),
    Review(ReviewReason),
    /// An archive document stays where it is, relative to its archive.
    Lies(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriedDocument {
    pub section: TrySection,
    /// Relative to its location.
    pub path: PathBuf,
    pub now: TryOutcome,
    pub after: TryOutcome,
    /// The other rules that match the document as well.
    pub conflicts: Vec<RuleId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TryResult {
    pub documents: Vec<TriedDocument>,
    /// Matches beyond [`MAX_TRIED_DOCUMENTS`].
    pub more: usize,
    /// The 1-based place of each conflicting rule on the rules page.
    pub conflict_names: HashMap<RuleId, usize>,
}

pub const MAX_TRIED_DOCUMENTS: usize = 50;

#[derive(Clone)]
pub struct RulesAccess {
    slot: IndexSlot,
    locations: Vec<Location>,
}

impl RulesAccess {
    pub fn new(slot: IndexSlot, locations: Vec<Location>) -> Self {
        Self { slot, locations }
    }

    /// Reads only: nothing is written to the index or the disk. The documents
    /// the rule would match, as they stand and as they would stand with it.
    pub fn try_rule(&self, candidate: &RuleTry) -> Result<TryResult, String> {
        self.with_index(|index| {
            let rules_now = index.rules()?;
            let spec = RuleSpec {
                confirmed: true,
                enabled: true,
                ..candidate.spec.clone()
            };
            let (rules_after, candidate_id) = with_candidate(&rules_now, candidate.replaces, spec);
            let aliases = index.confirmed_sender_aliases()?;
            let in_review: HashSet<_> = index
                .open_reviews()?
                .into_iter()
                .map(|entry| entry.document_id)
                .collect();
            let mut tried = Vec::new();
            for location in &self.locations {
                for (id, absolute) in index.documents_under(&location.path)? {
                    let Some(text) = index.document_text(id)? else {
                        continue;
                    };
                    let Some(document) = index.document(id)? else {
                        continue;
                    };
                    let manual_fields = index.field_origins(id)?.manual_fields();
                    let input = PlanInput {
                        text: &text,
                        fields: &document.fields,
                        path: Some(&absolute),
                        sender_aliases: &aliases,
                        manual_fields: &manual_fields,
                    };
                    let after = plan(&rules_after, &input);
                    let matching = matching_rules(&after);
                    if !matching.contains(&candidate_id) {
                        continue;
                    }
                    let path = absolute
                        .strip_prefix(&location.path)
                        .map_or_else(|_| absolute.clone(), std::path::Path::to_path_buf);
                    let archived = location.mode == LocationMode::Archive;
                    let now = if archived {
                        TryOutcome::Lies(path.clone())
                    } else {
                        outcome(&plan(&rules_now, &input))
                    };
                    let section = if archived {
                        TrySection::Archive
                    } else if in_review.contains(&id) {
                        TrySection::Review
                    } else {
                        TrySection::Inbox
                    };
                    tried.push(TriedDocument {
                        section,
                        path,
                        now,
                        after: outcome(&after),
                        conflicts: matching
                            .iter()
                            .copied()
                            .filter(|other| *other != candidate_id)
                            .collect(),
                    });
                }
            }
            tried.sort_by(|left, right| {
                (left.section, &left.path).cmp(&(right.section, &right.path))
            });
            let more = tried.len().saturating_sub(MAX_TRIED_DOCUMENTS);
            tried.truncate(MAX_TRIED_DOCUMENTS);
            Ok(TryResult {
                conflict_names: conflict_places(&rules_after, &tried),
                documents: tried,
                more,
            })
        })
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
            Ok(RulesLoad { rules })
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

    /// Index refusals, such as a regex that does not compile, come back as
    /// the text of the error.
    pub fn save(&self, save: RuleSave) -> Result<(), String> {
        let done = self.with_index(|index| match save {
            RuleSave::Create { spec, origin } => {
                index.create_rule_from(&spec, origin).map(|_| true)
            }
            RuleSave::Update(id, spec) => index.update_rule(id, &spec).map(|()| true),
            RuleSave::ConfirmSuggestion(id, spec) => index.confirm_suggested_rule(id, &spec),
        })?;
        if done {
            Ok(())
        } else {
            Err(NO_SUCH_SUGGESTION.to_owned())
        }
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

/// The stored rules with the candidate in the place of `replaces`, or appended
/// when there is nothing to replace; and the id the candidate has there.
fn with_candidate(
    stored: &[Rule],
    replaces: Option<RuleId>,
    spec: RuleSpec,
) -> (Vec<Rule>, RuleId) {
    let replaced = replaces.filter(|id| stored.iter().any(|rule| rule.id == *id));
    if let Some(id) = replaced {
        let rules = stored
            .iter()
            .map(|rule| {
                if rule.id == id {
                    Rule {
                        spec: spec.clone(),
                        ..rule.clone()
                    }
                } else {
                    rule.clone()
                }
            })
            .collect();
        return (rules, id);
    }
    let id = RuleId::unused_in(stored);
    let position = stored
        .iter()
        .map(|rule| rule.position)
        .max()
        .map_or(1, |last| last.saturating_add(1));
    let mut rules = stored.to_vec();
    rules.push(Rule {
        id,
        position,
        origin: RuleOrigin::Hand,
        spec,
    });
    (rules, id)
}

fn matching_rules(planning: &Planning) -> &[RuleId] {
    match planning {
        Planning::Plan(filing) => &filing.matching_rules,
        Planning::Review { matching_rules, .. } => matching_rules,
    }
}

fn outcome(planning: &Planning) -> TryOutcome {
    match planning {
        Planning::Plan(filing) => TryOutcome::Filed(filing.relative_path.clone()),
        Planning::Review { reason, .. } => TryOutcome::Review(*reason),
    }
}

/// Where the rules page lists each conflicting rule: it lists the confirmed
/// ones in the order of the index.
fn conflict_places(rules: &[Rule], tried: &[TriedDocument]) -> HashMap<RuleId, usize> {
    let listed: Vec<RuleId> = rules
        .iter()
        .filter(|rule| rule.spec.confirmed)
        .map(|rule| rule.id)
        .collect();
    tried
        .iter()
        .flat_map(|document| &document.conflicts)
        .filter_map(|id| {
            let place = listed.iter().position(|listed_id| listed_id == id)?;
            Some((*id, place.saturating_add(1)))
        })
        .collect()
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

    use chrono::{TimeZone as _, Utc};
    use ncrs_index::{
        Condition, ConditionKind, ContentHash, DocumentFields, DocumentId, DocumentSource,
        DocumentStatus, FieldOrigin, NewDocument, RuleConditions, RuleEvidence, RuleOrigin,
        RuleSpec, RuleSuggestion,
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
        inbox: Location,
        archive: Location,
    }

    fn fixture() -> Fixture {
        let dir = TempDir::new().unwrap();
        let index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        let slot: IndexSlot = Arc::new(Mutex::new(Some(index)));
        let place = |name: &str, mode| {
            let path = dir.path().join(name);
            std::fs::create_dir_all(&path).unwrap();
            Location::new(&path, mode, false).unwrap()
        };
        let inbox = place("inbox", LocationMode::Inbox);
        let archive = place("archive", LocationMode::Archive);
        Fixture {
            access: RulesAccess::new(Arc::clone(&slot), vec![inbox.clone(), archive.clone()]),
            _dir: dir,
            slot,
            inbox,
            archive,
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
    fn a_created_rule_is_listed_with_the_origin_hand() {
        let fixture = fixture();

        fixture
            .access
            .save(RuleSave::Create {
                spec: spec("rent"),
                origin: RuleOrigin::Hand,
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
            .save(RuleSave::ConfirmSuggestion(s, edited.clone()))
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
            .save(RuleSave::ConfirmSuggestion(s, spec("tax")))
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
            let id = index.create_rule_from(&stored, RuleOrigin::Filing).unwrap();
            index.create_rule(&spec("power")).unwrap();
            id
        });
        let before = fixture.access.load().unwrap().rules[0].clone();
        let changed = RuleSpec {
            conditions: RuleConditions::any_of(vec![Condition::new(
                ConditionKind::Sender,
                "Telekom",
            )]),
            filing_pattern: "{year}/{sender}".to_owned(),
            ..before.rule.spec.clone()
        };

        fixture
            .access
            .save(RuleSave::Update(id, changed.clone()))
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
            })
            .unwrap_err();

        assert!(!error.is_empty());
        assert!(fixture.access.load().unwrap().rules.is_empty());
    }

    #[test]
    fn without_an_index_the_work_fails_with_a_plain_message() {
        let access = RulesAccess::new(Arc::new(Mutex::new(None)), Vec::new());

        assert_eq!(access.load().unwrap_err(), INDEX_NOT_OPEN);
    }

    fn filing_spec(word: &str, pattern: &str) -> RuleSpec {
        RuleSpec {
            filing_pattern: pattern.to_owned(),
            ..spec(word)
        }
    }

    fn candidate(replaces: Option<RuleId>, word: &str, pattern: &str) -> RuleTry {
        RuleTry {
            replaces,
            spec: filing_spec(word, pattern),
        }
    }

    impl Fixture {
        fn document(&self, location: &Location, name: &str, text: &str) -> DocumentId {
            let seed = u8::try_from(name.len()).unwrap();
            let mut hash = [seed; 32];
            hash[1..name.len().min(32)].copy_from_slice(&name.as_bytes()[1..name.len().min(32)]);
            self.with_index(|index| {
                let id = index
                    .insert_document(&NewDocument {
                        hash: ContentHash(hash),
                        path: location.path.join(name),
                        source: if location.mode == LocationMode::Archive {
                            DocumentSource::Archive
                        } else {
                            DocumentSource::Inbox
                        },
                        received_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
                        stamp: None,
                    })
                    .unwrap();
                index.set_document_text(id, text).unwrap();
                id
            })
        }

        fn snapshot(&self) -> (Vec<Rule>, Vec<ncrs_index::Document>, Vec<String>) {
            self.with_index(|index| {
                let mut documents = Vec::new();
                let mut origins = Vec::new();
                for location in [&self.inbox, &self.archive] {
                    for (id, _) in index.documents_under(&location.path).unwrap() {
                        documents.push(index.document(id).unwrap().unwrap());
                        origins.push(format!("{:?}", index.field_origins(id).unwrap()));
                    }
                }
                (index.rules().unwrap(), documents, origins)
            })
        }
    }

    #[test]
    fn a_new_rule_lists_the_documents_it_would_file_with_now_and_after() {
        let fixture = fixture();
        fixture.document(&fixture.inbox, "rent.pdf", "rent for march");
        fixture.document(&fixture.inbox, "other.pdf", "something else");
        let reviewed = fixture.document(&fixture.inbox, "rent-late.pdf", "rent again");
        fixture
            .with_index(|index| index.add_review(reviewed, ReviewReason::NoMatchingRule))
            .unwrap();
        fixture.document(&fixture.archive, "old/rent.pdf", "rent 2020");

        let result = fixture
            .access
            .try_rule(&candidate(None, "rent", "{kind}"))
            .unwrap();

        let listed: Vec<_> = result
            .documents
            .iter()
            .map(|tried| {
                (
                    tried.section,
                    tried.path.to_str().unwrap(),
                    &tried.now,
                    &tried.after,
                )
            })
            .collect();
        let filed = TryOutcome::Filed(PathBuf::from("Invoice"));
        let unplaced = TryOutcome::Review(ReviewReason::NoMatchingRule);
        let old_rent = PathBuf::from("old").join("rent.pdf");
        assert_eq!(
            listed,
            vec![
                (TrySection::Inbox, "rent.pdf", &unplaced, &filed),
                (TrySection::Review, "rent-late.pdf", &unplaced, &filed),
                (
                    TrySection::Archive,
                    old_rent.to_str().unwrap(),
                    &TryOutcome::Lies(old_rent.clone()),
                    &filed
                ),
            ]
        );
        assert_eq!(result.more, 0);
    }

    #[test]
    fn a_second_confirmed_rule_that_matches_is_a_conflict_with_its_place() {
        let fixture = fixture();
        fixture
            .with_index(|index| index.create_rule(&filing_spec("power", "{kind}")))
            .unwrap();
        let other = fixture
            .with_index(|index| index.create_rule(&filing_spec("rent", "{kind}/other")))
            .unwrap();
        fixture.document(&fixture.inbox, "rent.pdf", "rent");

        let result = fixture
            .access
            .try_rule(&candidate(None, "rent", "{kind}"))
            .unwrap();

        let tried = result.documents.first().unwrap();
        assert_eq!(tried.conflicts, vec![other]);
        assert_eq!(
            tried.now,
            TryOutcome::Filed(PathBuf::from("Invoice").join("other"))
        );
        assert_eq!(tried.after, TryOutcome::Review(ReviewReason::RuleConflict));
        assert_eq!(result.conflict_names, HashMap::from([(other, 2)]));
    }

    #[test]
    fn editing_a_rule_tries_the_edited_conditions_not_the_stored_ones() {
        let fixture = fixture();
        let stored = fixture
            .with_index(|index| index.create_rule(&filing_spec("rent", "{kind}")))
            .unwrap();
        fixture.document(&fixture.inbox, "rent.pdf", "rent for march");
        fixture.document(&fixture.inbox, "power.pdf", "power bill");

        let result = fixture
            .access
            .try_rule(&candidate(Some(stored), "power", "{kind}/edited"))
            .unwrap();

        assert_eq!(result.documents.len(), 1);
        let tried = result.documents.first().unwrap();
        assert_eq!(tried.path, PathBuf::from("power.pdf"));
        assert_eq!(tried.now, TryOutcome::Review(ReviewReason::NoMatchingRule));
        assert_eq!(
            tried.after,
            TryOutcome::Filed(PathBuf::from("Invoice").join("edited"))
        );
        assert!(tried.conflicts.is_empty());
    }

    #[test]
    fn a_kind_set_by_hand_survives_in_the_path_after() {
        let fixture = fixture();
        let id = fixture.document(&fixture.inbox, "rent.pdf", "rent for march");
        fixture.with_index(|index| {
            let by_hand = DocumentFields {
                kind: Some("offer".to_owned()),
                ..DocumentFields::default()
            };
            index
                .set_document_fields(id, &by_hand, |_| FieldOrigin::Manual)
                .unwrap();
        });

        let result = fixture
            .access
            .try_rule(&candidate(None, "rent", "{kind}"))
            .unwrap();

        let tried = result.documents.first().unwrap();
        assert_eq!(tried.after, TryOutcome::Filed(PathBuf::from("offer")));
    }

    #[test]
    fn trying_a_rule_changes_nothing_in_the_index() {
        let fixture = fixture();
        let stored = fixture
            .with_index(|index| index.create_rule(&filing_spec("power", "{kind}")))
            .unwrap();
        let reviewed = fixture.document(&fixture.inbox, "rent.pdf", "rent for march");
        fixture
            .with_index(|index| index.add_review(reviewed, ReviewReason::NoMatchingRule))
            .unwrap();
        let planned = fixture.document(&fixture.inbox, "power.pdf", "power bill");
        fixture
            .with_index(|index| index.set_document_status(planned, DocumentStatus::Planned))
            .unwrap();
        fixture.document(&fixture.archive, "rent-2020.pdf", "rent");
        let before = fixture.snapshot();
        let reviews_before = fixture.with_index(|index| index.open_reviews().unwrap());

        fixture
            .access
            .try_rule(&candidate(None, "rent", "{kind}"))
            .unwrap();
        fixture
            .access
            .try_rule(&candidate(Some(stored), "rent", "{kind}"))
            .unwrap();

        assert_eq!(fixture.snapshot(), before);
        assert_eq!(
            fixture.with_index(|index| index.open_reviews().unwrap()),
            reviews_before
        );
    }

    #[test]
    fn documents_beyond_the_cap_are_counted_and_the_list_stays_in_order() {
        let fixture = fixture();
        for number in 0..=MAX_TRIED_DOCUMENTS {
            fixture.document(&fixture.inbox, &format!("rent-{number:03}.pdf"), "rent");
        }

        let result = fixture
            .access
            .try_rule(&candidate(None, "rent", "{kind}"))
            .unwrap();

        assert_eq!(result.documents.len(), MAX_TRIED_DOCUMENTS);
        assert_eq!(result.more, 1);
        let paths: Vec<_> = result
            .documents
            .iter()
            .map(|tried| tried.path.clone())
            .collect();
        let mut sorted = paths.clone();
        sorted.sort();
        assert_eq!(paths, sorted);
    }

    #[test]
    fn a_document_without_text_is_not_listed() {
        let fixture = fixture();
        fixture.document(&fixture.inbox, "scan.pdf", "rent");
        let blank = fixture.with_index(|index| {
            index
                .insert_document(&NewDocument {
                    hash: ContentHash([200; 32]),
                    path: fixture.inbox.path.join("blank.pdf"),
                    source: DocumentSource::Inbox,
                    received_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
                    stamp: None,
                })
                .unwrap()
        });
        assert!(fixture
            .with_index(|index| index.document_text(blank).unwrap())
            .is_none());

        let result = fixture
            .access
            .try_rule(&candidate(None, "rent", "{kind}"))
            .unwrap();

        assert_eq!(result.documents.len(), 1);
    }
}
