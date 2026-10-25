//! Files the inbox documents the rules have placed. It blocks, so `App` runs
//! it on a blocking thread, never in `update`.
//!
//! The place of a document is worked out again here from the current rules and
//! the stored text and fields, so a rule changed since the planning counts and
//! nothing about the plan has to be stored. The moving itself is the journaled
//! executor; nothing is overwritten or deleted.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use ncrs_index::{BatchKind, Document, DocumentStatus, Index, IndexError, ReviewReason, Rule};
use ncrs_intake::{plan, PlanInput, Planning};

use crate::execute::{execute_with, ExecuteError, ExecutionItem, ItemOutcome, SkipReason};
use crate::fs::transfer::{Move, Progress, Source};
use crate::suggestion_work::INDEX_NOT_OPEN;

type IndexSlot = Arc<Mutex<Option<Index>>>;

/// What the index may be asked for by the filing job: the planned documents
/// and the archive that is the filing target.
#[derive(Clone)]
pub struct FilingAccess {
    slot: IndexSlot,
    target_root: PathBuf,
}

impl FilingAccess {
    pub fn new(slot: IndexSlot, target_root: PathBuf) -> Self {
        Self { slot, target_root }
    }

    pub fn file_planned<P: Progress>(&self, progress: &mut P) -> Result<FilingSummary, String> {
        let mut guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let index = guard.as_mut().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        file_planned(index, &self.target_root, progress, &Move).map_err(|err| err.to_string())
    }
}

/// What a filing run did, by document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FilingSummary {
    pub filed: usize,
    /// Put in front of the user: the place is taken, or no rule places it now.
    pub reviewed: usize,
    /// The file changed or vanished; it goes through the pipeline again.
    pub requeued: usize,
    /// Still planned: stopped, or could not be moved this time.
    pub left: usize,
}

/// The name the rules gave, with the extension of the file: a filing pattern
/// describes the name without it.
fn target_of(root: &Path, relative: &Path, source: &Path) -> PathBuf {
    let mut name: OsString = relative.as_os_str().to_owned();
    if let Some(extension) = source.extension() {
        let has_it = relative
            .extension()
            .is_some_and(|own| own.eq_ignore_ascii_case(extension));
        if !has_it {
            name.push(".");
            name.push(extension);
        }
    }
    root.join(name)
}

enum Decision {
    Move(ExecutionItem),
    Reviewed,
}

fn decide(
    index: &mut Index,
    rules: &[Rule],
    root: &Path,
    document: &Document,
) -> Result<Decision, IndexError> {
    let text = index.document_text(document.id)?.unwrap_or_default();
    let planning = plan(
        rules,
        &PlanInput {
            text: &text,
            fields: &document.fields,
        },
    );
    match planning {
        Planning::Plan(filing) => {
            if filing.fields != document.fields {
                index.set_document_fields(document.id, &filing.fields)?;
            }
            Ok(Decision::Move(ExecutionItem {
                from: document.path.clone(),
                to: target_of(root, &filing.relative_path, &document.path),
                expected_hash: document.hash,
                document_id: Some(document.id),
                duplicate: false,
            }))
        }
        Planning::Review { reason, .. } => {
            send_to_review(index, document, reason)?;
            Ok(Decision::Reviewed)
        }
    }
}

/// A review entry that is already open is kept; the status is what the
/// filing job depends on.
fn send_to_review(
    index: &mut Index,
    document: &Document,
    reason: ReviewReason,
) -> Result<(), IndexError> {
    match index.add_review(document.id, reason) {
        Ok(_) => Ok(()),
        Err(IndexError::ReviewAlreadyOpen(_)) => {
            index.set_document_status(document.id, DocumentStatus::InReview)
        }
        Err(other) => Err(other),
    }
}

/// Files every `Planned` document into `root`.
pub fn file_planned<P: Progress, S: Source>(
    index: &mut Index,
    root: &Path,
    progress: &mut P,
    effect: &S,
) -> Result<FilingSummary, ExecuteError> {
    let rules = index.rules()?;
    let mut summary = FilingSummary::default();
    let mut items = Vec::new();
    for id in index.documents_with_status(DocumentStatus::Planned)? {
        let Some(document) = index.document(id)? else {
            continue;
        };
        match decide(index, &rules, root, &document)? {
            Decision::Move(item) => items.push(item),
            Decision::Reviewed => summary.reviewed += 1,
        }
    }
    if items.is_empty() {
        return Ok(summary);
    }
    let report = execute_with(index, BatchKind::Filing, root, &items, progress, effect)?;
    for (item, reported) in items.iter().zip(&report.items) {
        let Some(id) = item.document_id else {
            continue;
        };
        match &reported.outcome {
            ItemOutcome::Moved => summary.filed += 1,
            ItemOutcome::Skipped(SkipReason::TargetExists) => {
                if let Some(document) = index.document(id)? {
                    send_to_review(index, &document, ReviewReason::TargetExists)?;
                }
                summary.reviewed += 1;
            }
            ItemOutcome::Skipped(SkipReason::SourceChanged | SkipReason::SourceMissing) => {
                index.set_document_status(id, DocumentStatus::New)?;
                summary.requeued += 1;
            }
            ItemOutcome::Skipped(_) => summary.left += 1,
        }
    }
    Ok(summary)
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
    use std::collections::BTreeSet;
    use std::fs;
    use std::io;

    use chrono::{TimeZone as _, Utc};
    use ncrs_index::{
        DocumentSource, JournalState, NewDocument, NewJournalItem, RuleSpec, TaxContext, TextMatch,
    };
    use ncrs_intake::hash_path;
    use tempfile::TempDir;

    use super::*;
    use crate::execute::reconcile;
    use crate::fs::transfer::Counting;

    /// A move that always has to copy, as across volumes.
    struct AcrossVolumes;

    impl Source for AcrossVolumes {
        fn removes_source(&self) -> bool {
            true
        }
        fn remove(&self, path: &Path) -> io::Result<()> {
            Move.remove(path)
        }
        fn relocate(&self, _from: &Path, _to: &Path) -> io::Result<bool> {
            Ok(false)
        }
    }

    struct Setup {
        _dir: TempDir,
        inbox: PathBuf,
        archive: PathBuf,
        index: Index,
    }

    impl Setup {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let inbox = dir.path().join("inbox");
            let archive = dir.path().join("archive");
            fs::create_dir_all(&inbox).unwrap();
            fs::create_dir_all(&archive).unwrap();
            let mut index = Index::create_plain(&dir.path().join("index.db")).unwrap();
            index
                .create_rule(&RuleSpec {
                    match_text: Some(TextMatch::Contains("company a".to_owned())),
                    match_sender: None,
                    extract_number: None,
                    extract_date: None,
                    extract_amount: None,
                    kind: "invoice".to_owned(),
                    tax_context: TaxContext::Company,
                    tags: BTreeSet::new(),
                    filing_pattern: "Insurance/{kind}.pdf".to_owned(),
                    confirmed: true,
                })
                .unwrap();
            Self {
                _dir: dir,
                inbox,
                archive,
                index,
            }
        }

        /// An inbox file the rules have planned.
        fn planned(&mut self, name: &str, content: &str) -> Document {
            let path = self.inbox.join(name);
            fs::write(&path, content).unwrap();
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
            self.index
                .set_document_text(id, "Invoice from Company A")
                .unwrap();
            self.index
                .set_document_status(id, DocumentStatus::Planned)
                .unwrap();
            self.index.document(id).unwrap().unwrap()
        }

        fn run<S: Source>(&mut self, effect: &S) -> FilingSummary {
            file_planned(&mut self.index, &self.archive, &mut Counting, effect).unwrap()
        }

        fn target(&self) -> PathBuf {
            self.archive.join("Insurance").join("invoice.pdf")
        }
    }

    fn assert_filed(setup: &Setup, document: &Document) {
        let after = setup.index.document(document.id).unwrap().unwrap();
        assert_eq!(after.path, setup.target());
        assert_eq!(after.status, DocumentStatus::Filed);
        assert_eq!(
            fs::read_to_string(setup.target()).unwrap(),
            "Invoice from Company A"
        );
        assert!(!document.path.exists());
        let batch = setup
            .index
            .last_undoable_batch(ncrs_index::BatchKind::Filing)
            .unwrap()
            .unwrap();
        assert_eq!(
            setup.index.journal_items(batch).unwrap()[0].state,
            JournalState::Moved
        );
        let staging: Vec<_> = fs::read_dir(setup.archive.join("Insurance"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(staging, [std::ffi::OsString::from("invoice.pdf")]);
    }

    #[test]
    fn a_planned_document_is_filed_within_one_volume() {
        let mut setup = Setup::new();
        let document = setup.planned("scan 1.pdf", "Invoice from Company A");

        let summary = setup.run(&Move);

        assert_eq!(summary.filed, 1);
        assert_filed(&setup, &document);
        assert_eq!(
            setup
                .index
                .document_by_path(&setup.target())
                .unwrap()
                .map(|d| d.id),
            Some(document.id)
        );
    }

    #[test]
    fn a_planned_document_is_filed_across_the_volume_boundary() {
        let mut setup = Setup::new();
        let document = setup.planned("scan 1.pdf", "Invoice from Company A");

        let summary = setup.run(&AcrossVolumes);

        assert_eq!(summary.filed, 1);
        assert_filed(&setup, &document);
    }

    #[test]
    fn a_taken_target_sends_the_document_to_review_and_changes_nothing() {
        let mut setup = Setup::new();
        let document = setup.planned("scan 1.pdf", "Invoice from Company A");
        fs::create_dir_all(setup.archive.join("Insurance")).unwrap();
        fs::write(setup.target(), "Agency B").unwrap();

        let summary = setup.run(&Move);

        assert_eq!(summary.reviewed, 1);
        assert_eq!(fs::read_to_string(setup.target()).unwrap(), "Agency B");
        assert!(document.path.exists());
        let after = setup.index.document(document.id).unwrap().unwrap();
        assert_eq!(after.status, DocumentStatus::InReview);
        assert_eq!(after.path, document.path);
        let reviews = setup.index.open_reviews().unwrap();
        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0].document_id, document.id);
        assert_eq!(reviews[0].reason, ReviewReason::TargetExists);
    }

    #[test]
    fn a_rule_that_no_longer_places_the_document_sends_it_to_review() {
        let mut setup = Setup::new();
        let document = setup.planned("scan 1.pdf", "Invoice from Company A");
        for rule in setup.index.rules().unwrap() {
            setup.index.delete_rule(rule.id).unwrap();
        }

        let summary = setup.run(&Move);

        assert_eq!(summary.reviewed, 1);
        assert!(document.path.exists());
        assert_eq!(
            setup.index.open_reviews().unwrap()[0].reason,
            ReviewReason::NoMatchingRule
        );
    }

    #[test]
    fn a_file_edited_after_the_planning_goes_back_to_new() {
        let mut setup = Setup::new();
        let document = setup.planned("scan 1.pdf", "Invoice from Company A");
        fs::write(&document.path, "edited").unwrap();

        let summary = setup.run(&Move);

        assert_eq!(summary.requeued, 1);
        assert!(!setup.target().exists());
        let after = setup.index.document(document.id).unwrap().unwrap();
        assert_eq!(after.status, DocumentStatus::New);
    }

    /// The app stopped after the copy was made and before the source was
    /// removed: the journal says `pending`, both files exist.
    #[test]
    fn a_crash_between_the_copy_and_the_delete_ends_with_one_file_at_the_target() {
        let mut setup = Setup::new();
        let document = setup.planned("scan 1.pdf", "Invoice from Company A");
        fs::create_dir_all(setup.archive.join("Insurance")).unwrap();
        fs::copy(&document.path, setup.target()).unwrap();
        let batch = setup
            .index
            .begin_batch(ncrs_index::BatchKind::Filing)
            .unwrap();
        setup
            .index
            .add_journal_item(
                batch,
                &NewJournalItem {
                    from: document.path.clone(),
                    to: setup.target(),
                    hash: document.hash,
                    document_id: Some(document.id),
                    duplicate: false,
                    state: JournalState::Pending,
                    reason: None,
                },
            )
            .unwrap();

        assert_eq!(reconcile(&setup.index).unwrap(), 1);
        let summary = setup.run(&Move);

        assert_eq!(summary, FilingSummary::default());
        assert_filed(&setup, &document);
        assert_eq!(
            setup.index.documents_under(&setup.archive).unwrap().len(),
            1
        );
        assert_eq!(setup.index.documents_under(&setup.inbox).unwrap().len(), 0);
        assert!(setup.index.duplicate_paths(document.id).unwrap().is_empty());
    }
}
