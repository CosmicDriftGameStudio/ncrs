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
    BatchKind, Document, DocumentId, DocumentStatus, FieldOrigin, Index, IndexError, KindLanguage,
    Kinds, ReviewEntry, ReviewEntryId, ReviewReason, Rule, SenderAliases,
};
use ncrs_intake::{plan, PlanInput, Planning};

use crate::execute::{execute_with, ExecuteError, ExecutionItem, ItemOutcome, SkipReason};
use crate::filing_work::{send_to_review, target_of};
use crate::suggestion_work::INDEX_NOT_OPEN;
use ncrs_fs::transfer::{Move, Progress, Source};

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
    kind_language: KindLanguage,
}

impl std::fmt::Debug for ReplanAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReplanAccess").finish_non_exhaustive()
    }
}

impl ReplanAccess {
    pub fn new(slot: IndexSlot, inbox_roots: Vec<PathBuf>, archive_root: PathBuf) -> Self {
        Self {
            slot,
            inbox_roots,
            archive_root,
            kind_language: KindLanguage::default(),
        }
    }

    pub fn with_kind_language(self, kind_language: KindLanguage) -> Self {
        Self {
            kind_language,
            ..self
        }
    }

    pub fn preview(&self, scope: &ReplanScope) -> Result<Vec<ReplanItem>, String> {
        let mut guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let index = guard.as_mut().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        preview(index, scope, &self.inbox_roots, self.kind_language).map_err(|err| err.to_string())
    }

    pub fn file_previewed<P: Progress>(
        &self,
        items: &[ReplanItem],
        progress: &mut P,
    ) -> Result<ReplanSummary, String> {
        let mut guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let index = guard.as_mut().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        file_previewed(
            index,
            &self.archive_root,
            items,
            progress,
            &Move,
            self.kind_language,
        )
        .map_err(|err| err.to_string())
    }
}

/// The rules and the confirmed sender aliases a planning needs, read once for
/// a run.
struct Rulebook {
    rules: Vec<Rule>,
    sender_aliases: SenderAliases,
    kinds: Kinds,
}

impl Rulebook {
    fn load(index: &Index, kind_language: KindLanguage) -> Result<Self, IndexError> {
        Ok(Self {
            rules: index.rules()?,
            sender_aliases: index.confirmed_sender_aliases()?,
            kinds: Kinds::new(Arc::clone(index.kinds()), kind_language),
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
                kinds: &self.kinds,
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
    kind_language: KindLanguage,
) -> Result<Vec<ReplanItem>, IndexError> {
    let rulebook = Rulebook::load(index, kind_language)?;
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
    kind_language: KindLanguage,
) -> Result<ReplanSummary, ExecuteError> {
    let rulebook = Rulebook::load(index, kind_language)?;
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
#[cfg(any(test, feature = "test-support"))]
pub mod fixtures;
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests;
