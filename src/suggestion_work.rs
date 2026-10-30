//! The work behind the rule suggestions view. Everything here blocks, so `App`
//! runs it on a blocking thread, never in `update`.
//!
//! The index is shared with the intake, which holds the same mutex while it
//! files a document. So the index is locked in short steps: documents and
//! texts are read under the lock, the analysis runs without it, and the result
//! is written under it again.
//!
//! The text of a document is data. It only ever becomes a literal "text
//! contains" condition that the user confirms; nothing here acts on it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use ncrs_index::{Index, IndexError, RuleEvidence, RuleId, RuleSpec};
use ncrs_intake::{
    contains_condition_holds, learn_archive, suggest_rules, ArchiveDocument, ArchiveText,
};

type IndexSlot = Arc<Mutex<Option<Index>>>;

pub const INDEX_NOT_OPEN: &str = "index not open";

/// A rule the analysis suggested and the user has not decided on.
#[derive(Debug, Clone, PartialEq)]
pub struct SuggestedRule {
    pub id: RuleId,
    pub spec: RuleSpec,
    pub evidence: RuleEvidence,
}

/// A document of an archive location with the text the index holds for it.
struct ArchiveEntry {
    document: ArchiveDocument,
    text: Option<String>,
}

/// What the view may do with the index: the suggestions and the documents they
/// match, nothing else.
#[derive(Clone)]
pub struct SuggestionAccess {
    slot: IndexSlot,
    /// The folders of the archive locations as the intake sees them: the
    /// index stores the paths found below these.
    archive_roots: Vec<PathBuf>,
}

impl SuggestionAccess {
    pub fn new(slot: IndexSlot, archive_roots: Vec<PathBuf>) -> Self {
        Self {
            slot,
            archive_roots,
        }
    }

    /// Learns every archive location, suggests rules from it and returns all
    /// open suggestions.
    pub fn analyse(&self) -> Result<Vec<SuggestedRule>, String> {
        for root in &self.archive_roots {
            let entries = self.with_index(|index| archive_entries(index, root))?;
            let documents: Vec<ArchiveDocument> =
                entries.iter().map(|entry| entry.document.clone()).collect();
            let texts: Vec<ArchiveText> = entries
                .into_iter()
                .filter_map(|entry| {
                    entry.text.map(|text| ArchiveText {
                        document: entry.document,
                        text,
                    })
                })
                .collect();
            let profile = learn_archive(&documents);
            let candidates = suggest_rules(&texts, &profile);
            self.with_index(|index| {
                index.replace_location_profile(root, &profile)?;
                for candidate in candidates {
                    index.suggest_rule(&candidate.spec, candidate.evidence)?;
                }
                Ok(())
            })?;
        }
        self.load()
    }

    pub fn load(&self) -> Result<Vec<SuggestedRule>, String> {
        self.with_index(|index| {
            Ok(index
                .suggested_rules()?
                .into_iter()
                .map(|(rule, evidence)| SuggestedRule {
                    id: rule.id,
                    spec: rule.spec,
                    evidence,
                })
                .collect())
        })
    }

    /// Confirms the suggestion with the condition, kind and pattern of `spec`
    /// (the edited ones, or the suggested ones) and returns the open rest.
    pub fn confirm(&self, id: RuleId, spec: &RuleSpec) -> Result<Vec<SuggestedRule>, String> {
        self.with_index(|index| index.confirm_suggested_rule(id, spec).map(drop))?;
        self.load()
    }

    /// Discards the suggestion for good and returns the open rest.
    pub fn discard(&self, id: RuleId) -> Result<Vec<SuggestedRule>, String> {
        self.with_index(|index| index.discard_rule_suggestion(id).map(drop))?;
        self.load()
    }

    /// The documents of the archive locations whose text a "contains"
    /// condition finds, below their location, sorted.
    pub fn matching_documents(&self, condition: &str) -> Result<Vec<PathBuf>, String> {
        let mut found = Vec::new();
        for root in &self.archive_roots {
            let entries = self.with_index(|index| archive_entries(index, root))?;
            found.extend(entries.into_iter().filter_map(|entry| {
                let text = entry.text?;
                contains_condition_holds(&text.to_lowercase(), condition)
                    .then_some(entry.document.relative_path)
            }));
        }
        found.sort();
        Ok(found)
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

fn archive_entries(index: &Index, root: &Path) -> Result<Vec<ArchiveEntry>, IndexError> {
    let mut entries = Vec::new();
    for (id, path) in index.documents_under(root)? {
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let date = index
            .document(id)?
            .and_then(|document| document.fields.date);
        entries.push(ArchiveEntry {
            document: ArchiveDocument {
                relative_path: relative.to_path_buf(),
                date,
            },
            text: index
                .document_text(id)?
                .filter(|text| !text.trim().is_empty()),
        });
    }
    Ok(entries)
}

/// The condition of a suggestion as text; `None` for a rule that has none or
/// a pattern as its condition, which the suggestions never have.
pub fn condition_text(spec: &RuleSpec) -> Option<&str> {
    spec.conditions.single_text()
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use chrono::{TimeZone as _, Utc};
    use ncrs_index::{
        Condition, ConditionKind, ContentHash, DocumentSource, NewDocument, RuleConditions,
        RuleSuggestion,
    };
    use tempfile::TempDir;

    use super::*;

    struct Archive {
        _dir: TempDir,
        root: PathBuf,
        slot: IndexSlot,
        next_seed: u8,
    }

    impl Archive {
        fn access(&self) -> SuggestionAccess {
            SuggestionAccess::new(Arc::clone(&self.slot), vec![self.root.clone()])
        }

        fn add(&mut self, relative: &str, text: &str) {
            self.next_seed += 1;
            let mut guard = self.slot.lock().unwrap();
            let index = guard.as_mut().unwrap();
            let id = index
                .insert_document(&NewDocument {
                    hash: ContentHash([self.next_seed; 32]),
                    path: self.root.join(relative),
                    source: DocumentSource::Inbox,
                    received_at: Utc.with_ymd_and_hms(2026, 3, 14, 9, 30, 0).unwrap(),
                    stamp: None,
                })
                .unwrap();
            index.set_document_text(id, text).unwrap();
        }
    }

    /// Five insurance letters of Company A and five tax notices of Agency B.
    fn archive() -> Archive {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("Documents");
        let index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        let mut archive = Archive {
            _dir: dir,
            root,
            slot: Arc::new(Mutex::new(Some(index))),
            next_seed: 0,
        };
        for number in 0..5 {
            archive.add(
                &format!("Insurance/2024-01-{:02}_item{number}.pdf", number + 1),
                &format!(
                    "Company A Ltd\npolicy {}\nplease keep this letter",
                    1000 + number
                ),
            );
            archive.add(
                &format!("Taxes/2024-02-{:02}_item{number}.pdf", number + 1),
                &format!(
                    "Agency B Tax Office\nnotice {}\nplease keep this letter",
                    2000 + number
                ),
            );
        }
        archive
    }

    fn company_a(suggestions: &[SuggestedRule]) -> Option<&SuggestedRule> {
        suggestions
            .iter()
            .find(|suggestion| condition_text(&suggestion.spec) == Some("company a ltd"))
    }

    #[test]
    fn analysing_suggests_rules_with_their_evidence() {
        let archive = archive();

        let suggestions = archive.access().analyse().unwrap();

        let suggestion = company_a(&suggestions).expect("the Company A rule");
        assert!(!suggestion.spec.confirmed);
        assert_eq!(suggestion.spec.kind, "Insurance");
        assert_eq!(
            suggestion.evidence,
            RuleEvidence {
                matching_in_folder: 5,
                folder_documents: 5,
                matching_elsewhere: 0,
            }
        );
        let learned = archive
            .slot
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .location_profile(&archive.root)
            .unwrap();
        assert!(!learned.is_empty(), "the profile is stored");
    }

    #[test]
    fn analysing_again_keeps_one_suggestion_per_rule() {
        let archive = archive();
        let first = archive.access().analyse().unwrap();

        let second = archive.access().analyse().unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn confirming_sets_the_rule_confirmed_and_takes_it_off_the_list() {
        let archive = archive();
        let access = archive.access();
        let suggestion = company_a(&access.analyse().unwrap()).unwrap().clone();

        let rest = access.confirm(suggestion.id, &suggestion.spec).unwrap();

        assert!(company_a(&rest).is_none());
        let guard = archive.slot.lock().unwrap();
        let rule = guard
            .as_ref()
            .unwrap()
            .rule(suggestion.id)
            .unwrap()
            .unwrap();
        assert!(rule.spec.confirmed);
        assert_eq!(rule.spec.conditions, suggestion.spec.conditions);
    }

    #[test]
    fn confirming_an_edit_stores_the_edited_rule() {
        let archive = archive();
        let access = archive.access();
        let suggestion = company_a(&access.analyse().unwrap()).unwrap().clone();
        let edited = RuleSpec {
            conditions: RuleConditions::all_of(vec![Condition::new(
                ConditionKind::Substring,
                "company a",
            )]),
            kind: "Policy".to_owned(),
            filing_pattern: "Policies/{date}_{name}".to_owned(),
            ..suggestion.spec.clone()
        };

        access.confirm(suggestion.id, &edited).unwrap();
        let after_rerun = access.analyse().unwrap();

        let guard = archive.slot.lock().unwrap();
        let rule = guard
            .as_ref()
            .unwrap()
            .rule(suggestion.id)
            .unwrap()
            .unwrap();
        assert!(rule.spec.confirmed);
        assert_eq!(rule.spec.conditions, edited.conditions);
        assert_eq!(rule.spec.kind, "Policy");
        assert_eq!(rule.spec.filing_pattern, "Policies/{date}_{name}");
        assert!(
            company_a(&after_rerun).is_none(),
            "the suggestion the edit replaced does not come back"
        );
    }

    #[test]
    fn a_discarded_suggestion_is_hidden_and_a_new_analysis_does_not_bring_it_back() {
        let archive = archive();
        let access = archive.access();
        let suggestion = company_a(&access.analyse().unwrap()).unwrap().clone();

        let rest = access.discard(suggestion.id).unwrap();
        let after_rerun = access.analyse().unwrap();

        assert!(company_a(&rest).is_none());
        assert!(company_a(&after_rerun).is_none());
        let agency = after_rerun
            .iter()
            .any(|other| condition_text(&other.spec) == Some("agency b tax office"));
        assert!(agency, "the other suggestions stay");
        let again = archive
            .slot
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .suggest_rule(&suggestion.spec, suggestion.evidence)
            .unwrap();
        assert_eq!(again, RuleSuggestion::Discarded);
    }

    #[test]
    fn the_matching_documents_are_the_ones_the_condition_finds() {
        let archive = archive();

        let found = archive
            .access()
            .matching_documents("COMPANY A ltd")
            .unwrap();

        assert_eq!(found.len(), 5);
        assert!(found.iter().all(|path| path.starts_with("Insurance")));
        assert!(archive.access().matching_documents("").unwrap().is_empty());
        assert!(archive
            .access()
            .matching_documents("nothing of the kind")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn without_an_open_index_the_work_reports_it() {
        let archive = archive();
        *archive.slot.lock().unwrap() = None;

        assert_eq!(
            archive.access().analyse().unwrap_err(),
            INDEX_NOT_OPEN.to_owned()
        );
    }
}
