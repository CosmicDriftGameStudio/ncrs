use chrono::Utc;
use rusqlite::{params, OptionalExtension as _};

use crate::error::IndexError;
use crate::index::Index;
use crate::model::{
    Condition, ConditionKind, Rule, RuleEvidence, RuleId, RuleSpec, RuleSuggestion,
};

impl Index {
    /// Stores a rule that was suggested from the archive. It is always stored
    /// unconfirmed, whatever `spec.confirmed` says. A rule with the same
    /// condition, kind and filing pattern is not duplicated: an unconfirmed
    /// one gets the new evidence, a confirmed one is left untouched. A
    /// condition and target the user discarded is not stored again.
    pub fn suggest_rule(
        &mut self,
        spec: &RuleSpec,
        evidence: RuleEvidence,
    ) -> Result<RuleSuggestion, IndexError> {
        let single = spec.conditions.single_condition();
        let existing = match single {
            Some(condition) => self.same_condition_and_target(spec, condition)?,
            None => None,
        };
        let discarded = match single {
            Some(condition) if existing.is_none() => self.was_discarded(condition, spec)?,
            _ => false,
        };
        if discarded {
            return Ok(RuleSuggestion::Discarded);
        }
        match existing {
            Some((id, true)) => Ok(RuleSuggestion::AlreadyConfirmed(id)),
            Some((id, false)) => {
                self.store_rule_evidence(id, &evidence)?;
                Ok(RuleSuggestion::Refreshed(id))
            }
            None => {
                let unconfirmed = RuleSpec {
                    confirmed: false,
                    ..spec.clone()
                };
                let id = self.create_rule(&unconfirmed)?;
                self.store_rule_evidence(id, &evidence)?;
                Ok(RuleSuggestion::Created(id))
            }
        }
    }

    /// The evidence a rule was suggested with; `None` for rules made by hand.
    pub fn rule_evidence(&self, id: RuleId) -> Result<Option<RuleEvidence>, IndexError> {
        let found = self
            .connection()
            .query_row(
                "SELECT matching_in_folder, folder_documents, matching_elsewhere, subtree
                 FROM rule_evidence WHERE rule_id = ?1",
                [id.get()],
                |row| {
                    Ok(RuleEvidence {
                        matching_in_folder: row.get(0)?,
                        folder_documents: row.get(1)?,
                        matching_elsewhere: row.get(2)?,
                        subtree: row.get(3)?,
                    })
                },
            )
            .optional()?;
        Ok(found)
    }

    /// The suggested rules the user has not decided on, with the evidence
    /// they were suggested with, ordered by id.
    pub fn suggested_rules(&self) -> Result<Vec<(Rule, RuleEvidence)>, IndexError> {
        let mut suggested = Vec::new();
        for rule in self.rules()? {
            if rule.spec.confirmed {
                continue;
            }
            if let Some(evidence) = self.rule_evidence(rule.id)? {
                suggested.push((rule, evidence));
            }
        }
        Ok(suggested)
    }

    /// Confirms a suggested rule, with the values of `edited` for condition,
    /// kind and filing pattern. When those differ from the suggestion, the
    /// suggestion is remembered as discarded: it was replaced, so a new
    /// analysis must not bring it back beside the edited rule. `false` when
    /// there is no unconfirmed rule with this id.
    pub fn confirm_suggested_rule(
        &mut self,
        id: RuleId,
        edited: &RuleSpec,
    ) -> Result<bool, IndexError> {
        let Some(rule) = self.rule(id)? else {
            return Ok(false);
        };
        if rule.spec.confirmed {
            return Ok(false);
        }
        let confirmed = RuleSpec {
            confirmed: true,
            ..edited.clone()
        };
        self.update_rule(id, &confirmed)?;
        let changed = rule.spec.conditions != confirmed.conditions
            || rule.spec.kind != confirmed.kind
            || rule.spec.filing_pattern != confirmed.filing_pattern;
        if changed {
            self.remember_discarded(&rule.spec)?;
        }
        Ok(true)
    }

    /// Removes a suggested rule and remembers its condition and target, so
    /// `suggest_rule` does not propose them again. `false` when there is no
    /// unconfirmed rule with a single condition under this id.
    pub fn discard_rule_suggestion(&mut self, id: RuleId) -> Result<bool, IndexError> {
        let Some(rule) = self.rule(id)? else {
            return Ok(false);
        };
        if rule.spec.confirmed || !self.remember_discarded(&rule.spec)? {
            return Ok(false);
        }
        // The evidence goes with the rule (ON DELETE CASCADE).
        self.delete_rule(id)
    }

    /// `false` when the rule has no single condition to remember.
    fn remember_discarded(&self, spec: &RuleSpec) -> Result<bool, IndexError> {
        let Some(condition) = spec.conditions.single_condition() else {
            return Ok(false);
        };
        self.connection().execute(
            "INSERT OR IGNORE INTO discarded_rule_suggestion
                 (match_text, condition_kind, kind, filing_pattern, discarded_at)
             VALUES (lower(?1), ?2, lower(?3), ?4, ?5)",
            params![
                condition.value,
                discard_class(condition.kind),
                spec.kind,
                spec.filing_pattern,
                Utc::now().timestamp()
            ],
        )?;
        Ok(true)
    }

    fn was_discarded(&self, condition: &Condition, spec: &RuleSpec) -> Result<bool, IndexError> {
        let found = self.connection().query_row(
            "SELECT EXISTS (SELECT 1 FROM discarded_rule_suggestion
                 WHERE match_text = lower(?1) AND condition_kind = ?2
                     AND kind = lower(?3) AND filing_pattern = ?4)",
            params![
                condition.value,
                discard_class(condition.kind),
                spec.kind,
                spec.filing_pattern
            ],
            |row| row.get(0),
        )?;
        Ok(found)
    }

    /// A confirmed match is preferred over an unconfirmed one.
    fn same_condition_and_target(
        &self,
        spec: &RuleSpec,
        text_condition: &Condition,
    ) -> Result<Option<(RuleId, bool)>, IndexError> {
        let found = self
            .connection()
            .query_row(
                "SELECT r.id, r.confirmed FROM rule r
                 JOIN document_kind k ON k.id = r.kind_id
                 JOIN rule_condition c ON c.rule_id = r.id
                 WHERE (SELECT count(*) FROM rule_condition WHERE rule_id = r.id) = 1
                     AND c.negated = 0 AND c.kind = ?4 AND lower(c.value) = lower(?1)
                     AND k.name = ?2 AND r.filing_pattern = ?3
                 ORDER BY r.confirmed DESC, r.id LIMIT 1",
                params![
                    text_condition.value,
                    spec.kind,
                    spec.filing_pattern,
                    text_condition.kind.as_str()
                ],
                |row| Ok((RuleId::new(row.get(0)?), row.get(1)?)),
            )
            .optional()?;
        Ok(found)
    }

    fn store_rule_evidence(&self, id: RuleId, evidence: &RuleEvidence) -> Result<(), IndexError> {
        self.connection().execute(
            "INSERT INTO rule_evidence
                 (rule_id, matching_in_folder, folder_documents, matching_elsewhere, subtree,
                  updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (rule_id) DO UPDATE SET
                 matching_in_folder = excluded.matching_in_folder,
                 folder_documents = excluded.folder_documents,
                 matching_elsewhere = excluded.matching_elsewhere,
                 subtree = excluded.subtree,
                 updated_at = excluded.updated_at",
            params![
                id.get(),
                evidence.matching_in_folder,
                evidence.folder_documents,
                evidence.matching_elsewhere,
                evidence.subtree,
                Utc::now().timestamp()
            ],
        )?;
        Ok(())
    }
}

/// Word and substring conditions are one class when discarding: the
/// suggestions only ever made words, and an edit may turn one into the other.
fn discard_class(kind: ConditionKind) -> &'static str {
    match kind {
        ConditionKind::Substring | ConditionKind::Word => ConditionKind::Word.as_str(),
        other => other.as_str(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::model::{ConditionKind, RuleConditions};
    use crate::test_support::{encrypted_index, rule_spec};

    fn text_condition(text: &str) -> RuleConditions {
        RuleConditions::all_of(vec![Condition::new(ConditionKind::Substring, text)])
    }

    fn suggestion() -> RuleSpec {
        RuleSpec {
            conditions: text_condition("company a ltd"),
            confirmed: true,
            ..rule_spec()
        }
    }

    fn evidence(matching: u32, elsewhere: u32) -> RuleEvidence {
        RuleEvidence {
            matching_in_folder: matching,
            folder_documents: 5,
            matching_elsewhere: elsewhere,
            subtree: Some("Insurance".to_owned()),
        }
    }

    #[test]
    fn a_suggested_rule_is_stored_unconfirmed_with_its_evidence() {
        let (_dir, mut index) = encrypted_index();

        let stored = index.suggest_rule(&suggestion(), evidence(5, 0)).unwrap();

        let RuleSuggestion::Created(id) = stored else {
            panic!("{stored:?}");
        };
        assert!(!index.rule(id).unwrap().unwrap().spec.confirmed);
        assert_eq!(index.rule_evidence(id).unwrap(), Some(evidence(5, 0)));
    }

    #[test]
    fn suggesting_again_updates_the_evidence_instead_of_adding_a_rule() {
        let (_dir, mut index) = encrypted_index();
        let RuleSuggestion::Created(id) =
            index.suggest_rule(&suggestion(), evidence(4, 1)).unwrap()
        else {
            panic!("not created");
        };

        let again = index.suggest_rule(&suggestion(), evidence(5, 0)).unwrap();

        assert_eq!(again, RuleSuggestion::Refreshed(id));
        assert_eq!(index.rules().unwrap().len(), 1);
        assert_eq!(index.rule_evidence(id).unwrap(), Some(evidence(5, 0)));
    }

    #[test]
    fn a_confirmed_rule_stays_confirmed_and_keeps_its_values() {
        let (_dir, mut index) = encrypted_index();
        let mut confirmed = suggestion();
        confirmed.tax_context = crate::TaxContext::Private;
        let id = index.create_rule(&confirmed).unwrap();
        let before = index.rule(id).unwrap();

        let result = index.suggest_rule(&suggestion(), evidence(5, 0)).unwrap();

        assert_eq!(result, RuleSuggestion::AlreadyConfirmed(id));
        assert_eq!(index.rule(id).unwrap(), before);
        assert_eq!(index.rules().unwrap().len(), 1);
        assert_eq!(index.rule_evidence(id).unwrap(), None);
    }

    #[test]
    fn a_discarded_suggestion_is_gone_and_not_proposed_again() {
        let (_dir, mut index) = encrypted_index();
        let RuleSuggestion::Created(id) =
            index.suggest_rule(&suggestion(), evidence(5, 0)).unwrap()
        else {
            panic!("not created");
        };

        assert!(index.discard_rule_suggestion(id).unwrap());
        let again = index.suggest_rule(&suggestion(), evidence(5, 0)).unwrap();

        assert_eq!(again, RuleSuggestion::Discarded);
        assert!(index.rules().unwrap().is_empty());
        assert_eq!(index.rule_evidence(id).unwrap(), None);
        let shouting = RuleSpec {
            conditions: text_condition("COMPANY A LTD"),
            ..suggestion()
        };
        assert_eq!(
            index.suggest_rule(&shouting, evidence(5, 0)).unwrap(),
            RuleSuggestion::Discarded,
            "the condition is compared without case"
        );
    }

    #[test]
    fn a_discarded_condition_is_only_discarded_for_its_target() {
        let (_dir, mut index) = encrypted_index();
        let RuleSuggestion::Created(id) =
            index.suggest_rule(&suggestion(), evidence(5, 0)).unwrap()
        else {
            panic!("not created");
        };
        index.discard_rule_suggestion(id).unwrap();
        let other_target = RuleSpec {
            filing_pattern: "{year}/Other".to_owned(),
            ..suggestion()
        };

        let stored = index.suggest_rule(&other_target, evidence(5, 0)).unwrap();

        assert!(matches!(stored, RuleSuggestion::Created(_)));
    }

    #[test]
    fn a_confirmed_rule_cannot_be_discarded() {
        let (_dir, mut index) = encrypted_index();
        let id = index.create_rule(&suggestion()).unwrap();

        assert!(!index.discard_rule_suggestion(id).unwrap());
        assert!(!index.discard_rule_suggestion(RuleId::new(99)).unwrap());

        assert_eq!(index.rules().unwrap().len(), 1);
    }

    #[test]
    fn the_list_has_the_unconfirmed_suggestions_with_their_evidence() {
        let (_dir, mut index) = encrypted_index();
        index.create_rule(&suggestion()).unwrap();
        let by_hand = RuleSpec {
            confirmed: false,
            conditions: text_condition("agency b"),
            ..suggestion()
        };
        index.create_rule(&by_hand).unwrap();
        let other = RuleSpec {
            conditions: text_condition("company c"),
            ..suggestion()
        };
        let RuleSuggestion::Created(id) = index.suggest_rule(&other, evidence(4, 1)).unwrap()
        else {
            panic!("not created");
        };

        let listed = index.suggested_rules().unwrap();

        let [(rule, shown)] = listed.as_slice() else {
            panic!("{listed:?}");
        };
        assert_eq!(rule.id, id);
        assert_eq!(*shown, evidence(4, 1));
    }

    #[test]
    fn confirming_sets_the_rule_confirmed_and_keeps_its_values() {
        let (_dir, mut index) = encrypted_index();
        let RuleSuggestion::Created(id) =
            index.suggest_rule(&suggestion(), evidence(5, 0)).unwrap()
        else {
            panic!("not created");
        };

        assert!(index.confirm_suggested_rule(id, &suggestion()).unwrap());

        let rule = index.rule(id).unwrap().unwrap();
        assert!(rule.spec.confirmed);
        assert_eq!(rule.spec.conditions, suggestion().conditions);
        assert!(index.suggested_rules().unwrap().is_empty());
        assert!(!index.confirm_suggested_rule(id, &suggestion()).unwrap());
    }

    #[test]
    fn confirming_an_edited_suggestion_stores_the_edit_and_does_not_bring_the_original_back() {
        let (_dir, mut index) = encrypted_index();
        let RuleSuggestion::Created(id) =
            index.suggest_rule(&suggestion(), evidence(5, 0)).unwrap()
        else {
            panic!("not created");
        };
        let edited = RuleSpec {
            conditions: text_condition("company a"),
            kind: "Letter".to_owned(),
            filing_pattern: "{year}/Letters".to_owned(),
            ..suggestion()
        };

        assert!(index.confirm_suggested_rule(id, &edited).unwrap());
        let again = index.suggest_rule(&suggestion(), evidence(5, 0)).unwrap();

        let rule = index.rule(id).unwrap().unwrap();
        assert!(rule.spec.confirmed);
        assert_eq!(rule.spec.conditions, edited.conditions);
        assert_eq!(rule.spec.kind, "Letter");
        assert_eq!(rule.spec.filing_pattern, "{year}/Letters");
        assert_eq!(again, RuleSuggestion::Discarded);
        assert_eq!(index.rules().unwrap().len(), 1);
    }

    #[test]
    fn another_target_is_another_rule() {
        let (_dir, mut index) = encrypted_index();
        index.suggest_rule(&suggestion(), evidence(5, 0)).unwrap();
        let other_target = RuleSpec {
            filing_pattern: "{year}/Other".to_owned(),
            ..suggestion()
        };

        let stored = index.suggest_rule(&other_target, evidence(5, 0)).unwrap();

        assert!(matches!(stored, RuleSuggestion::Created(_)));
        assert_eq!(index.rules().unwrap().len(), 2);
    }

    fn sender_condition(name: &str) -> RuleConditions {
        RuleConditions::all_of(vec![Condition::new(ConditionKind::Sender, name)])
    }

    fn sender_suggestion() -> RuleSpec {
        RuleSpec {
            conditions: sender_condition("Company A"),
            ..suggestion()
        }
    }

    #[test]
    fn a_sender_suggestion_is_not_duplicated_by_the_next_analysis() {
        let (_dir, mut index) = encrypted_index();
        let RuleSuggestion::Created(id) = index
            .suggest_rule(&sender_suggestion(), evidence(5, 0))
            .unwrap()
        else {
            panic!("not created");
        };

        let again = index
            .suggest_rule(&sender_suggestion(), evidence(4, 1))
            .unwrap();

        assert_eq!(again, RuleSuggestion::Refreshed(id));
        assert_eq!(index.rules().unwrap().len(), 1);
    }

    #[test]
    fn a_discarded_sender_suggestion_is_not_proposed_again() {
        let (_dir, mut index) = encrypted_index();
        let RuleSuggestion::Created(id) = index
            .suggest_rule(&sender_suggestion(), evidence(5, 0))
            .unwrap()
        else {
            panic!("not created");
        };

        assert!(index.discard_rule_suggestion(id).unwrap());
        let shouting = RuleSpec {
            conditions: sender_condition("COMPANY A"),
            ..sender_suggestion()
        };

        assert_eq!(
            index.suggest_rule(&shouting, evidence(5, 0)).unwrap(),
            RuleSuggestion::Discarded
        );
    }

    #[test]
    fn discarding_a_word_does_not_block_the_sender_of_the_same_name_and_the_other_way_round() {
        let (_dir, mut index) = encrypted_index();
        let word = RuleSpec {
            conditions: RuleConditions::all_of(vec![Condition::new(
                ConditionKind::Word,
                "company a",
            )]),
            ..suggestion()
        };
        let RuleSuggestion::Created(word_id) = index.suggest_rule(&word, evidence(5, 0)).unwrap()
        else {
            panic!("not created");
        };
        index.discard_rule_suggestion(word_id).unwrap();

        let RuleSuggestion::Created(sender_id) = index
            .suggest_rule(&sender_suggestion(), evidence(5, 0))
            .unwrap()
        else {
            panic!("the sender suggestion was blocked");
        };
        index.discard_rule_suggestion(sender_id).unwrap();

        assert_eq!(
            index.suggest_rule(&word, evidence(5, 0)).unwrap(),
            RuleSuggestion::Discarded
        );
        assert_eq!(
            index
                .suggest_rule(&sender_suggestion(), evidence(5, 0))
                .unwrap(),
            RuleSuggestion::Discarded
        );
    }

    #[test]
    fn editing_the_value_of_a_sender_suggestion_does_not_bring_the_original_back() {
        let (_dir, mut index) = encrypted_index();
        let RuleSuggestion::Created(id) = index
            .suggest_rule(&sender_suggestion(), evidence(5, 0))
            .unwrap()
        else {
            panic!("not created");
        };
        let edited = RuleSpec {
            conditions: sender_condition("Company A Ltd"),
            ..sender_suggestion()
        };

        assert!(index.confirm_suggested_rule(id, &edited).unwrap());

        assert_eq!(
            index
                .suggest_rule(&sender_suggestion(), evidence(5, 0))
                .unwrap(),
            RuleSuggestion::Discarded
        );
    }
}
