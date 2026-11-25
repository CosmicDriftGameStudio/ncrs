use std::collections::BTreeSet;
use std::path::PathBuf;

use chrono::{TimeZone as _, Utc};
use rusqlite::OptionalExtension as _;
use tempfile::TempDir;

use crate::{
    Condition, ConditionKind, ContentHash, DocumentSource, InMemoryKeyStore, Index, NewDocument,
    RuleConditions, RuleEvidence, RuleId, RuleSpec,
};

/// An encrypted index in a temporary directory that lives as long as the guard.
pub fn encrypted_index() -> (TempDir, Index) {
    let dir = TempDir::new().unwrap();
    let store = InMemoryKeyStore::empty();
    let (index, _) = Index::create_encrypted(&dir.path().join("index.db"), &store).unwrap();
    (dir, index)
}

pub fn plain_index() -> (TempDir, Index) {
    let dir = TempDir::new().unwrap();
    let index = Index::create_plain(&dir.path().join("index.db")).unwrap();
    (dir, index)
}

pub fn new_document(seed: u8) -> NewDocument {
    NewDocument {
        hash: ContentHash([seed; 32]),
        path: PathBuf::from(format!("inbox/scan-{seed}.pdf")),
        source: DocumentSource::Inbox,
        received_at: Utc.with_ymd_and_hms(2026, 3, 14, 9, 30, 0).unwrap(),
        stamp: None,
    }
}

pub fn rule_spec() -> RuleSpec {
    RuleSpec {
        name: None,
        conditions: RuleConditions::all_of(vec![
            Condition::new(ConditionKind::Substring, "Invoice"),
            Condition::new(ConditionKind::Sender, "Company A"),
        ]),
        extract_number: Some(r"No\. (\d+)".to_owned()),
        extract_date: None,
        extract_amount: None,
        kind: Some("invoice".to_owned()),
        tags: BTreeSet::from(["services".to_owned(), "2026".to_owned()]),
        filing_pattern: "{year}/{sender}".to_owned(),
        confirmed: false,
        enabled: true,
    }
}

/// The evidence a rule was suggested with; `None` for rules made by hand.
pub fn rule_evidence(index: &Index, id: RuleId) -> Option<RuleEvidence> {
    index
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
        .optional()
        .unwrap()
}

/// Test paths are written with `/`; the subtree range uses the platform separator.
pub fn native(path: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(path.replace('/', std::path::MAIN_SEPARATOR_STR))
}
