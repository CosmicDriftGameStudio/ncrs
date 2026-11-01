//! The index and the folders the replan tests and the app tests work on.

use std::collections::BTreeSet;
use std::fs;

use chrono::{TimeZone as _, Utc};
use ncrs_index::{Condition, ConditionKind, DocumentSource, NewDocument, RuleConditions, RuleSpec};
use ncrs_intake::hash_path;
use tempfile::TempDir;

use super::*;

pub struct Setup {
    pub temp_dir: TempDir,
    pub inbox: PathBuf,
    pub archive: PathBuf,
    pub index: Index,
}

impl Default for Setup {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Setup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Setup")
            .field("inbox", &self.inbox)
            .field("archive", &self.archive)
            .finish_non_exhaustive()
    }
}

impl Setup {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        // The index stores canonical paths; a temp dir is spelled through a link on macOS.
        let dir_path = dir.path().canonicalize().unwrap();
        let inbox = dir_path.join("inbox");
        let archive = dir_path.join("archive");
        fs::create_dir_all(&inbox).unwrap();
        fs::create_dir_all(&archive).unwrap();
        let index = Index::create_plain(&dir_path.join("index.db")).unwrap();
        Self {
            temp_dir: dir,
            inbox,
            archive,
            index,
        }
    }

    pub fn add_rule(&mut self) {
        self.index
            .create_rule(&RuleSpec {
                name: None,
                conditions: RuleConditions::all_of(vec![Condition::new(
                    ConditionKind::Substring,
                    "company a",
                )]),
                extract_number: None,
                extract_date: None,
                extract_amount: None,
                kind: Some("invoice".to_owned()),
                tags: BTreeSet::new(),
                filing_pattern: "Insurance/{kind}.pdf".to_owned(),
                confirmed: true,
                enabled: true,
            })
            .unwrap();
    }

    /// A file in the inbox that waits in review: no rule placed it.
    pub fn in_review(&mut self, relative: &str, text: &str) -> Document {
        let path = self.inbox.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, relative).unwrap();
        let metadata = fs::metadata(&path).unwrap();
        let id = self
            .index
            .insert_document(&NewDocument {
                hash: hash_path(&path).unwrap(),
                path: path.clone(),
                source: DocumentSource::Inbox,
                received_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
                stamp: Some(ncrs_index::FileStamp {
                    size: metadata.len(),
                    mtime: metadata.modified().unwrap(),
                }),
            })
            .unwrap();
        self.index.set_document_text(id, text).unwrap();
        self.index
            .add_review(id, ReviewReason::NoMatchingRule)
            .unwrap();
        self.index
            .set_document_status(id, DocumentStatus::InReview)
            .unwrap();
        self.index.document(id).unwrap().unwrap()
    }

    /// The index and the folders, with the temp dir they live in.
    pub fn into_parts(self) -> (TempDir, PathBuf, PathBuf, Index) {
        (self.temp_dir, self.inbox, self.archive, self.index)
    }
}
