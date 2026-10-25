use chrono::Utc;
use rusqlite::{params, OptionalExtension as _};

use crate::error::IndexError;
use crate::index::Index;
use crate::model::{RuleEvidence, RuleId, RuleSpec, RuleSuggestion, TextMatch};

impl Index {
    /// Stores a rule that was suggested from the archive. It is always stored
    /// unconfirmed, whatever `spec.confirmed` says. A rule with the same text
    /// condition, kind and filing pattern is not duplicated: an unconfirmed
    /// one gets the new evidence, a confirmed one is left untouched.
    pub fn suggest_rule(
        &mut self,
        spec: &RuleSpec,
        evidence: RuleEvidence,
    ) -> Result<RuleSuggestion, IndexError> {
        let existing = match &spec.match_text {
            Some(TextMatch::Contains(text)) => self.same_condition_and_target(spec, text)?,
            Some(TextMatch::Regex(_)) | None => None,
        };
        match existing {
            Some((id, true)) => Ok(RuleSuggestion::AlreadyConfirmed(id)),
            Some((id, false)) => {
                self.store_rule_evidence(id, evidence)?;
                Ok(RuleSuggestion::Refreshed(id))
            }
            None => {
                let unconfirmed = RuleSpec {
                    confirmed: false,
                    ..spec.clone()
                };
                let id = self.create_rule(&unconfirmed)?;
                self.store_rule_evidence(id, evidence)?;
                Ok(RuleSuggestion::Created(id))
            }
        }
    }

    /// The evidence a rule was suggested with; `None` for rules made by hand.
    pub fn rule_evidence(&self, id: RuleId) -> Result<Option<RuleEvidence>, IndexError> {
        let found = self
            .connection()
            .query_row(
                "SELECT matching_in_folder, folder_documents, matching_elsewhere
                 FROM rule_evidence WHERE rule_id = ?1",
                [id.get()],
                |row| {
                    Ok(RuleEvidence {
                        matching_in_folder: row.get(0)?,
                        folder_documents: row.get(1)?,
                        matching_elsewhere: row.get(2)?,
                    })
                },
            )
            .optional()?;
        Ok(found)
    }

    /// A confirmed match is preferred over an unconfirmed one.
    fn same_condition_and_target(
        &self,
        spec: &RuleSpec,
        text: &str,
    ) -> Result<Option<(RuleId, bool)>, IndexError> {
        let found = self
            .connection()
            .query_row(
                "SELECT r.id, r.confirmed FROM rule r
                 JOIN document_kind k ON k.id = r.kind_id
                 WHERE lower(r.match_text) = lower(?1) AND r.match_text_is_regex = 0
                     AND r.match_sender_id IS NULL AND k.name = ?2 AND r.filing_pattern = ?3
                 ORDER BY r.confirmed DESC, r.id LIMIT 1",
                params![text, spec.kind, spec.filing_pattern],
                |row| Ok((RuleId::new(row.get(0)?), row.get(1)?)),
            )
            .optional()?;
        Ok(found)
    }

    fn store_rule_evidence(&self, id: RuleId, evidence: RuleEvidence) -> Result<(), IndexError> {
        self.connection().execute(
            "INSERT INTO rule_evidence
                 (rule_id, matching_in_folder, folder_documents, matching_elsewhere, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (rule_id) DO UPDATE SET
                 matching_in_folder = excluded.matching_in_folder,
                 folder_documents = excluded.folder_documents,
                 matching_elsewhere = excluded.matching_elsewhere,
                 updated_at = excluded.updated_at",
            params![
                id.get(),
                evidence.matching_in_folder,
                evidence.folder_documents,
                evidence.matching_elsewhere,
                Utc::now().timestamp()
            ],
        )?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::test_support::{encrypted_index, rule_spec};

    fn suggestion() -> RuleSpec {
        RuleSpec {
            match_text: Some(TextMatch::Contains("company a ltd".to_owned())),
            match_sender: None,
            confirmed: true,
            ..rule_spec()
        }
    }

    fn evidence(matching: u32, elsewhere: u32) -> RuleEvidence {
        RuleEvidence {
            matching_in_folder: matching,
            folder_documents: 5,
            matching_elsewhere: elsewhere,
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
}
