#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use chrono::{TimeZone as _, Utc};
use tempfile::TempDir;

use crate::{
    Condition, ConditionKind, ContentHash, DocumentSource, InMemoryKeyStore, Index, NewDocument,
    RuleConditions, RuleSpec, TaxContext,
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
        conditions: RuleConditions::all_of(vec![
            Condition::new(ConditionKind::Substring, "Invoice"),
            Condition::new(ConditionKind::Sender, "Company A"),
        ]),
        extract_number: Some(r"No\. (\d+)".to_owned()),
        extract_date: None,
        extract_amount: None,
        kind: "Invoice".to_owned(),
        tax_context: TaxContext::Company,
        tags: BTreeSet::from(["services".to_owned(), "2026".to_owned()]),
        filing_pattern: "{year}/{sender}".to_owned(),
        confirmed: false,
        enabled: true,
    }
}
