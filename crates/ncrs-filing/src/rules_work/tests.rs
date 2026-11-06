#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::BTreeSet;

use chrono::{TimeZone as _, Utc};
use ncrs_index::{
    Condition, ConditionKind, ContentHash, DocumentFields, DocumentId, DocumentSource,
    DocumentStatus, FieldOrigin, NewDocument, RuleConditions, RuleEvidence, RuleOrigin, RuleSpec,
    RuleSuggestion,
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
        conditions: RuleConditions::any_of(vec![Condition::new(ConditionKind::Sender, "Telekom")]),
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
    let filed = TryOutcome::Filed(PathBuf::from("Invoice.pdf"));
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
fn an_earlier_rule_that_matches_as_well_files_first_and_is_named_with_its_place() {
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
        TryOutcome::Filed(PathBuf::from("Invoice").join("other.pdf"))
    );
    assert_eq!(tried.after, tried.now);
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
        TryOutcome::Filed(PathBuf::from("Invoice").join("edited.pdf"))
    );
    assert!(tried.conflicts.is_empty());
}

#[test]
fn a_rule_on_tags_tries_the_own_tags_of_each_document() {
    let fixture = fixture();
    let tagged = fixture.document(&fixture.inbox, "tagged.pdf", "no word of the rule");
    fixture.document(&fixture.inbox, "untagged.pdf", "no word of the rule");
    fixture.with_index(|index| {
        let fields = DocumentFields {
            sender: Some("Firma A GmbH".to_owned()),
            tags: BTreeSet::from(["rechnung".to_owned()]),
            ..DocumentFields::default()
        };
        index
            .set_document_fields(tagged, &fields, |_| FieldOrigin::Manual)
            .unwrap();
    });
    let on_tags = RuleTry {
        replaces: None,
        spec: RuleSpec {
            conditions: RuleConditions::all_of(vec![
                Condition::new(ConditionKind::Tag, "rechnung"),
                Condition::new(ConditionKind::Tag, "firma-a"),
            ]),
            ..filing_spec("unused", "Tags/{kind}")
        },
    };

    let result = fixture.access.try_rule(&on_tags).unwrap();

    assert_eq!(result.documents.len(), 1);
    let tried = result.documents.first().unwrap();
    assert_eq!(tried.path, PathBuf::from("tagged.pdf"));
    assert_eq!(
        tried.after,
        TryOutcome::Filed(PathBuf::from("Tags").join("Invoice.pdf"))
    );
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
    assert_eq!(tried.after, TryOutcome::Filed(PathBuf::from("offer.pdf")));
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
