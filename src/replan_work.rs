//! Plans the open cases and the documents of an inbox folder again with the
//! current rules, shows what would change, and files exactly the documents
//! the user was shown. It blocks, so `App` runs it on a blocking thread, never
//! in `update`.
//!
//! The preview reads only. The filing plans every document once more right
//! before it moves: what differs from the preview stays where it is, so
//! nothing moves that the user did not see.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use ncrs_index::{
    BatchKind, Document, DocumentId, DocumentStatus, FieldOrigin, Index, IndexError, ReviewEntry,
    ReviewEntryId, ReviewReason, Rule, SenderAliases,
};
use ncrs_intake::{plan, PlanInput, Planning};

use crate::execute::{execute_with, ExecuteError, ExecutionItem, ItemOutcome, SkipReason};
use crate::filing_work::{send_to_review, target_of};
use crate::fs::transfer::{Move, Progress, Source};
use crate::suggestion_work::INDEX_NOT_OPEN;

type IndexSlot = Arc<Mutex<Option<Index>>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplanScope {
    /// Every document with an open review entry.
    OpenCases,
    /// The documents below this folder, which has to lie in an inbox location.
    Folder(PathBuf),
}

/// Where the current rules would put a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplanAfter {
    /// Below the archive.
    Filed(PathBuf),
    Review(ReviewReason),
    /// No text is stored, so the rules cannot judge it yet.
    Unread,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplanItem {
    pub document_id: DocumentId,
    pub review: Option<ReviewEntryId>,
    pub path: PathBuf,
    /// Why it is in review now; `None` when it is not.
    pub now: Option<ReviewReason>,
    pub after: ReplanAfter,
}

impl ReplanItem {
    pub fn would_be_filed(&self) -> bool {
        matches!(self.after, ReplanAfter::Filed(_))
    }
}

/// What filing the previewed documents did, by document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReplanSummary {
    pub filed: usize,
    /// Put in front of the user: the place is taken.
    pub reviewed: usize,
    /// The plan differed from the preview, so the document was not touched.
    pub changed: usize,
    /// The file changed or vanished; it goes through the pipeline again.
    pub requeued: usize,
    /// Not moved: filed meanwhile, excluded, gone, stopped or failed.
    pub left: usize,
}

/// Whether `directory` lies inside one of the inbox locations. Both sides are
/// made absolute but no link is followed: the stored document paths are what
/// counts. `absolute` keeps `..`, which would pass a component-wise prefix
/// test while pointing outside, so a path with one is refused.
pub fn lies_in_inbox(directory: &Path, inbox_roots: &[PathBuf]) -> bool {
    let Ok(absolute_directory) = std::path::absolute(directory) else {
        return false;
    };
    if absolute_directory
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return false;
    }
    inbox_roots.iter().any(|root| {
        std::path::absolute(root)
            .is_ok_and(|absolute_root| absolute_directory.starts_with(absolute_root))
    })
}

/// What the re-planning may ask of the index and the disk.
#[derive(Clone)]
pub struct ReplanAccess {
    slot: IndexSlot,
    inbox_roots: Vec<PathBuf>,
    archive_root: PathBuf,
}

impl ReplanAccess {
    pub fn new(slot: IndexSlot, inbox_roots: Vec<PathBuf>, archive_root: PathBuf) -> Self {
        Self {
            slot,
            inbox_roots,
            archive_root,
        }
    }

    pub fn preview(&self, scope: &ReplanScope) -> Result<Vec<ReplanItem>, String> {
        let mut guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let index = guard.as_mut().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        preview(index, scope, &self.inbox_roots).map_err(|err| err.to_string())
    }

    pub fn file_previewed<P: Progress>(
        &self,
        items: &[ReplanItem],
        progress: &mut P,
    ) -> Result<ReplanSummary, String> {
        let mut guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let index = guard.as_mut().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        file_previewed(index, &self.archive_root, items, progress, &Move)
            .map_err(|err| err.to_string())
    }
}

/// The rules and the confirmed sender aliases a planning needs, read once for
/// a run.
struct Rulebook {
    rules: Vec<Rule>,
    sender_aliases: SenderAliases,
}

impl Rulebook {
    fn load(index: &Index) -> Result<Self, IndexError> {
        Ok(Self {
            rules: index.rules()?,
            sender_aliases: index.confirmed_sender_aliases()?,
        })
    }

    /// The one planning of a stored document, for the preview and again right
    /// before the move, so a difference between the two is a real change.
    /// Fields the user set by hand are kept. `None` without stored text.
    fn plan(&self, index: &Index, document: &Document) -> Result<Option<Planning>, IndexError> {
        let Some(text) = index.document_text(document.id)? else {
            return Ok(None);
        };
        let manual_fields = index.field_origins(document.id)?.manual_fields();
        Ok(Some(plan(
            &self.rules,
            &PlanInput {
                text: &text,
                fields: &document.fields,
                path: Some(&document.path),
                sender_aliases: &self.sender_aliases,
                manual_fields: &manual_fields,
            },
        )))
    }
}

fn after_of(planning: Option<&Planning>) -> ReplanAfter {
    match planning {
        None => ReplanAfter::Unread,
        Some(Planning::Plan(filing)) => ReplanAfter::Filed(filing.relative_path.clone()),
        Some(Planning::Review { reason, .. }) => ReplanAfter::Review(*reason),
    }
}

/// The documents the scope names, each with its open review entry.
fn candidates(
    index: &Index,
    scope: &ReplanScope,
    inbox_roots: &[PathBuf],
) -> Result<Vec<(Document, Option<ReviewEntry>)>, IndexError> {
    let mut found = Vec::new();
    match scope {
        ReplanScope::OpenCases => {
            for entry in index.open_reviews()? {
                if let Some(document) = index.document(entry.document_id)? {
                    if document.missing_since.is_none() {
                        found.push((document, Some(entry)));
                    }
                }
            }
        }
        ReplanScope::Folder(directory) => {
            if !lies_in_inbox(directory, inbox_roots) {
                return Ok(found);
            }
            let Ok(absolute) = std::path::absolute(directory) else {
                return Ok(found);
            };
            let mut open: HashMap<DocumentId, ReviewEntry> = index
                .open_reviews()?
                .into_iter()
                .map(|entry| (entry.document_id, entry))
                .collect();
            for (id, _path) in index.documents_under(&absolute)? {
                let Some(document) = index.document(id)? else {
                    continue;
                };
                if document.status != DocumentStatus::Filed {
                    found.push((document, open.remove(&id)));
                }
            }
        }
    }
    Ok(found)
}

/// Reads only: nothing is written to the index or the disk. A folder outside
/// the inbox locations yields nothing; `lies_in_inbox` tells the caller why.
pub fn preview(
    index: &Index,
    scope: &ReplanScope,
    inbox_roots: &[PathBuf],
) -> Result<Vec<ReplanItem>, IndexError> {
    let rulebook = Rulebook::load(index)?;
    let mut items = Vec::new();
    for (document, entry) in candidates(index, scope, inbox_roots)? {
        // Excluded: Hermes leaves it where it is.
        if index.is_excluded(&document.path)? {
            continue;
        }
        let planning = rulebook.plan(index, &document)?;
        items.push(ReplanItem {
            document_id: document.id,
            review: entry.as_ref().map(|open| open.id),
            now: entry.as_ref().map(|open| open.reason),
            after: after_of(planning.as_ref()),
            path: document.path,
        });
    }
    Ok(items)
}

/// Files the previewed documents that would be filed, through the journaled
/// executor, after planning each one again.
pub fn file_previewed<P: Progress, S: Source>(
    index: &mut Index,
    archive_root: &Path,
    items: &[ReplanItem],
    progress: &mut P,
    effect: &S,
) -> Result<ReplanSummary, ExecuteError> {
    let rulebook = Rulebook::load(index)?;
    let mut summary = ReplanSummary::default();
    let mut moves = Vec::new();
    let mut moved_ids = Vec::new();
    for item in items {
        let ReplanAfter::Filed(expected) = &item.after else {
            continue;
        };
        let Some(document) = index.document(item.document_id)? else {
            summary.left += 1;
            continue;
        };
        if document.status == DocumentStatus::Filed
            || document.missing_since.is_some()
            || index.is_excluded(&document.path)?
        {
            summary.left += 1;
            continue;
        }
        if document.path != item.path {
            summary.changed += 1;
            continue;
        }
        let Some(Planning::Plan(filing)) = rulebook.plan(index, &document)? else {
            summary.changed += 1;
            continue;
        };
        if filing.relative_path != *expected {
            summary.changed += 1;
            continue;
        }
        if filing.fields != document.fields {
            let manual_fields = index.field_origins(document.id)?.manual_fields();
            index.set_document_fields(document.id, &filing.fields, |field| {
                if manual_fields.contains(&field) {
                    FieldOrigin::Manual
                } else {
                    FieldOrigin::Rule(Some(filing.rule))
                }
            })?;
        }
        moves.push(ExecutionItem {
            from: document.path.clone(),
            to: target_of(archive_root, &filing.relative_path, &document.path),
            expected_hash: document.hash,
            document_id: Some(document.id),
            duplicate: false,
        });
        moved_ids.push(document.id);
    }
    if moves.is_empty() {
        return Ok(summary);
    }
    let report = execute_with(
        index,
        BatchKind::Filing,
        archive_root,
        &moves,
        progress,
        effect,
    )?;
    let open: HashMap<DocumentId, ReviewEntryId> = index
        .open_reviews()?
        .into_iter()
        .map(|entry| (entry.document_id, entry.id))
        .collect();
    for (id, reported) in moved_ids.iter().zip(&report.items) {
        match &reported.outcome {
            ItemOutcome::Moved => {
                if let Some(entry) = open.get(id) {
                    index.resolve_review(*entry)?;
                }
                summary.filed += 1;
            }
            ItemOutcome::Skipped(SkipReason::TargetExists) => {
                if let Some(document) = index.document(*id)? {
                    send_to_review(index, &document, ReviewReason::TargetExists)?;
                }
                summary.reviewed += 1;
            }
            ItemOutcome::Skipped(SkipReason::SourceChanged | SkipReason::SourceMissing) => {
                if let Some(entry) = open.get(id) {
                    index.resolve_review(*entry)?;
                }
                index.set_document_status(*id, DocumentStatus::New)?;
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
pub mod tests {
    use std::collections::BTreeSet;
    use std::fs;

    use chrono::{TimeZone as _, Utc};
    use ncrs_index::{
        Condition, ConditionKind, DocumentFields, DocumentSource, Field, NewDocument,
        RuleConditions, RuleSpec,
    };
    use ncrs_intake::hash_path;
    use tempfile::TempDir;

    use super::*;
    use crate::fs::transfer::Counting;

    pub struct Setup {
        pub temp_dir: TempDir,
        pub inbox: PathBuf,
        pub archive: PathBuf,
        pub index: Index,
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
                    conditions: RuleConditions::all_of(vec![Condition::new(
                        ConditionKind::Substring,
                        "company a",
                    )]),
                    extract_number: None,
                    extract_date: None,
                    extract_amount: None,
                    kind: "invoice".to_owned(),
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

        fn preview(&self, scope: &ReplanScope) -> Vec<ReplanItem> {
            preview(&self.index, scope, std::slice::from_ref(&self.inbox)).unwrap()
        }

        fn file(&mut self, items: &[ReplanItem]) -> ReplanSummary {
            file_previewed(&mut self.index, &self.archive, items, &mut Counting, &Move).unwrap()
        }

        fn target(&self) -> PathBuf {
            self.archive.join("Insurance").join("invoice.pdf")
        }
    }

    const INVOICE_TEXT: &str = "Invoice from Company A";

    #[test]
    fn open_cases_are_planned_again_after_a_new_rule() {
        let mut setup = Setup::new();
        let document = setup.in_review("scan.pdf", INVOICE_TEXT);

        let before = setup.preview(&ReplanScope::OpenCases);
        assert!(before
            .iter()
            .all(|item| item.after == ReplanAfter::Review(ReviewReason::NoMatchingRule)));

        setup.add_rule();
        let after = setup.preview(&ReplanScope::OpenCases);
        let item = after
            .iter()
            .find(|item| item.document_id == document.id)
            .unwrap();
        assert_eq!(item.now, Some(ReviewReason::NoMatchingRule));
        assert!(item.review.is_some());
        assert_eq!(
            item.after,
            ReplanAfter::Filed(PathBuf::from("Insurance").join("invoice.pdf"))
        );
    }

    #[test]
    fn a_document_without_stored_text_is_unread() {
        let mut setup = Setup::new();
        setup.add_rule();
        let path = setup.inbox.join("blank.pdf");
        fs::write(&path, "blank").unwrap();
        let id = setup
            .index
            .insert_document(&NewDocument {
                hash: hash_path(&path).unwrap(),
                path,
                source: DocumentSource::Inbox,
                received_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
                stamp: None,
            })
            .unwrap();
        setup
            .index
            .add_review(id, ReviewReason::NoMatchingRule)
            .unwrap();

        let items = setup.preview(&ReplanScope::OpenCases);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].after, ReplanAfter::Unread);
    }

    #[test]
    fn the_preview_writes_nothing() {
        let mut setup = Setup::new();
        let document = setup.in_review("scan.pdf", INVOICE_TEXT);
        setup.add_rule();
        let origins = setup.index.field_origins(document.id).unwrap();

        let items = setup.preview(&ReplanScope::OpenCases);
        assert_eq!(items.len(), 1);

        let unchanged = setup.index.document(document.id).unwrap().unwrap();
        assert_eq!(unchanged, document);
        assert_eq!(setup.index.field_origins(document.id).unwrap(), origins);
        assert_eq!(setup.index.open_reviews().unwrap().len(), 1);
        assert!(document.path.exists());
        assert!(!setup.target().exists());
        assert!(setup
            .index
            .last_undoable_batch(BatchKind::Filing)
            .unwrap()
            .is_none());
    }

    #[test]
    fn filing_the_previewed_moves_the_file_and_settles_the_review() {
        let mut setup = Setup::new();
        let document = setup.in_review("scan.pdf", INVOICE_TEXT);
        setup.add_rule();
        let items = setup.preview(&ReplanScope::OpenCases);

        let summary = setup.file(&items);

        assert_eq!(
            summary,
            ReplanSummary {
                filed: 1,
                ..ReplanSummary::default()
            }
        );
        let after = setup.index.document(document.id).unwrap().unwrap();
        assert_eq!(after.path, setup.target());
        assert_eq!(after.status, DocumentStatus::Filed);
        assert_eq!(fs::read_to_string(setup.target()).unwrap(), "scan.pdf");
        assert!(!document.path.exists());
        assert!(setup.index.open_reviews().unwrap().is_empty());
        assert!(setup
            .index
            .last_undoable_batch(BatchKind::Filing)
            .unwrap()
            .is_some());
    }

    #[test]
    fn a_document_whose_plan_changed_since_the_preview_is_not_moved() {
        let mut setup = Setup::new();
        let document = setup.in_review("scan.pdf", INVOICE_TEXT);
        setup.add_rule();
        let items = setup.preview(&ReplanScope::OpenCases);
        setup
            .index
            .set_document_text(document.id, "Something else")
            .unwrap();

        let summary = setup.file(&items);

        assert_eq!(
            summary,
            ReplanSummary {
                changed: 1,
                ..ReplanSummary::default()
            }
        );
        assert!(document.path.exists());
        assert!(!setup.target().exists());
        assert_eq!(setup.index.open_reviews().unwrap().len(), 1);
        assert!(setup
            .index
            .last_undoable_batch(BatchKind::Filing)
            .unwrap()
            .is_none());
    }

    #[test]
    fn only_the_documents_the_preview_would_file_are_moved() {
        let mut setup = Setup::new();
        let filed = setup.in_review("a.pdf", INVOICE_TEXT);
        let stays = setup.in_review("b.pdf", "Nothing known");
        setup.add_rule();
        let items = setup.preview(&ReplanScope::OpenCases);
        assert_eq!(items.iter().filter(|item| item.would_be_filed()).count(), 1);

        let summary = setup.file(&items);

        assert_eq!(summary.filed, 1);
        assert!(!filed.path.exists());
        assert!(stays.path.exists());
        assert_eq!(setup.index.open_reviews().unwrap().len(), 1);
    }

    #[test]
    fn a_manual_kind_survives_the_filing() {
        let mut setup = Setup::new();
        let document = setup.in_review("scan.pdf", INVOICE_TEXT);
        setup.add_rule();
        setup
            .index
            .set_document_fields(
                document.id,
                &DocumentFields {
                    kind: Some("contract".to_owned()),
                    ..DocumentFields::default()
                },
                |_| FieldOrigin::Manual,
            )
            .unwrap();
        let items = setup.preview(&ReplanScope::OpenCases);
        assert_eq!(
            items[0].after,
            ReplanAfter::Filed(PathBuf::from("Insurance").join("contract.pdf"))
        );

        let summary = setup.file(&items);

        assert_eq!(summary.filed, 1);
        let after = setup.index.document(document.id).unwrap().unwrap();
        assert_eq!(
            after.path,
            setup.archive.join("Insurance").join("contract.pdf")
        );
        assert_eq!(after.fields.kind.as_deref(), Some("contract"));
        assert_eq!(
            setup
                .index
                .field_origins(document.id)
                .unwrap()
                .of(Field::Kind),
            FieldOrigin::Manual
        );
    }

    #[test]
    fn a_folder_outside_the_inbox_is_refused() {
        let mut setup = Setup::new();
        setup.in_review("scan.pdf", INVOICE_TEXT);
        setup.add_rule();
        let elsewhere = setup.archive.clone();

        assert!(!lies_in_inbox(
            &elsewhere,
            std::slice::from_ref(&setup.inbox)
        ));
        assert!(setup.preview(&ReplanScope::Folder(elsewhere)).is_empty());
        assert!(lies_in_inbox(
            &setup.inbox.join("deeper"),
            std::slice::from_ref(&setup.inbox)
        ));
        assert!(!lies_in_inbox(
            &setup.inbox.join("..").join("archive"),
            std::slice::from_ref(&setup.inbox)
        ));
    }

    #[test]
    fn a_folder_scope_takes_only_the_documents_below_it() {
        let mut setup = Setup::new();
        let inside = setup.in_review("sub/a.pdf", INVOICE_TEXT);
        setup.in_review("sub2/b.pdf", INVOICE_TEXT);
        setup.in_review("c.pdf", INVOICE_TEXT);
        setup.add_rule();

        let items = setup.preview(&ReplanScope::Folder(setup.inbox.join("sub")));

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].document_id, inside.id);
        assert!(items[0].would_be_filed());
    }

    #[test]
    fn a_folder_scope_takes_documents_not_in_review_but_not_the_filed_ones() {
        let mut setup = Setup::new();
        let waiting = setup.in_review("sub/a.pdf", INVOICE_TEXT);
        let done = setup.in_review("sub/b.pdf", INVOICE_TEXT);
        setup
            .index
            .set_document_status(done.id, DocumentStatus::Filed)
            .unwrap();
        setup.add_rule();

        let items = setup.preview(&ReplanScope::Folder(setup.inbox.join("sub")));

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].document_id, waiting.id);
    }
}
