//! Carries out a list of moves inside an archive location, with a journal so
//! an interrupted run can be settled and a finished one undone.
//!
//! Nothing here deletes a file the user has: the only things removed are the
//! source of a move whose copy was checked, the copies a copy run made and
//! the empty folders a run created itself. A run never overwrites; a source
//! whose content is not the one that was planned is left alone.
//!
//! A run moves, copies or trashes (`Transfer`). A copy leaves its source and
//! the index paths alone; the intake records the copy when it sees it.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ncrs_index::{
    BatchId, BatchKind, ContentHash, DocumentId, DocumentStatus, FileStamp, Index, IndexError,
    JournalItem, JournalState, NewJournalItem, ReviewReason, SourceRoot, Transfer,
};
use ncrs_intake::{has_symlink_ancestor_under, hash_path, stays_below, IntakeError};

use ncrs_fs::transfer::{sweep_partials, transfer_to, Counting, Move, OnConflict, Source};

mod respell;
mod run_steps;
mod trash;
pub use respell::is_respelling;
use respell::{is_same_entry, respell, respell_back, restore_respelled};
pub use run_steps::{open_run_batch, run_one};
pub use trash::{trash_can_restore, trash_run, trash_run_own, OwnTrashItem, TrashItem, TrashReport};

/// One move. Paths are absolute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionItem {
    pub from: PathBuf,
    pub to: PathBuf,
    /// What the file at `from` must hold; anything else is skipped.
    pub expected_hash: ContentHash,
    /// The document the index knows the file as.
    pub document_id: Option<DocumentId>,
    /// `from` is a copy recorded in `duplicate_path`, not the document's file.
    pub duplicate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    OutsideLocation,
    /// A folder between the root and the path is a link that may lead out.
    LeavesLocation,
    SourceChanged,
    SourceMissing,
    TargetExists,
    Cancelled,
    /// Stopped before it was settled; the file is where it was.
    Interrupted,
    Failed(String),
}

impl SkipReason {
    /// What the journal stores.
    pub fn text(&self) -> String {
        match self {
            Self::OutsideLocation => "outside_location".to_owned(),
            Self::LeavesLocation => "leaves_location".to_owned(),
            Self::SourceChanged => "source_changed".to_owned(),
            Self::SourceMissing => "source_missing".to_owned(),
            Self::TargetExists => "target_exists".to_owned(),
            Self::Cancelled => "cancelled".to_owned(),
            Self::Interrupted => "interrupted".to_owned(),
            Self::Failed(detail) => format!("failed: {detail}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemOutcome {
    Moved,
    Skipped(SkipReason),
}

#[derive(Debug)]
pub enum ExecuteError {
    Index(IndexError),
    Io { path: PathBuf, source: io::Error },
}

impl fmt::Display for ExecuteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Index(err) => err.fmt(f),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl std::error::Error for ExecuteError {}

impl From<IndexError> for ExecuteError {
    fn from(err: IndexError) -> Self {
        Self::Index(err)
    }
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> ExecuteError + '_ {
    move |source| ExecuteError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// The folders a run works between.
#[derive(Debug, Clone, Copy)]
pub struct RunPlaces<'a> {
    pub source: &'a Path,
    pub target: &'a Path,
    /// The items may also come from below `target`: a run renames what
    /// already lies there in the same batch as the files it brings in.
    pub inside_target: bool,
}

pub(in crate::execute) fn remember_source_root(source: &Path) -> Result<SourceRoot, ExecuteError> {
    Ok(SourceRoot {
        given: source.to_path_buf(),
        resolved: ncrs_intake::canonical_path(source).map_err(io_error(source))?,
    })
}

fn journal_entry(
    item: &ExecutionItem,
    state: JournalState,
    reason: Option<String>,
) -> NewJournalItem {
    NewJournalItem {
        from: item.from.clone(),
        to: item.to.clone(),
        hash: item.expected_hash,
        document_id: item.document_id,
        duplicate: item.duplicate,
        state,
        reason,
    }
}

fn skip(
    index: &Index,
    batch: BatchId,
    item: &ExecutionItem,
    reason: SkipReason,
) -> Result<ItemOutcome, ExecuteError> {
    index.add_journal_item(
        batch,
        &journal_entry(item, JournalState::Skipped, Some(reason.text())),
    )?;
    Ok(ItemOutcome::Skipped(reason))
}

fn is_below(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root).is_ok_and(stays_below)
}

fn is_not_found(err: &IntakeError) -> bool {
    matches!(err, IntakeError::Io { source, .. } if source.kind() == io::ErrorKind::NotFound)
}

fn run_item<S: Source>(
    index: &Index,
    batch: BatchId,
    transfer: Transfer,
    places: &RunPlaces<'_>,
    item: &ExecutionItem,
    effect: &S,
) -> Result<ItemOutcome, ExecuteError> {
    let root = places.target;
    if !is_below(root, &item.to) {
        return skip(index, batch, item, SkipReason::OutsideLocation);
    }
    match has_symlink_ancestor_under(root, &item.to) {
        Ok(false) => {}
        Ok(true) => return skip(index, batch, item, SkipReason::LeavesLocation),
        Err(err) => return skip(index, batch, item, SkipReason::Failed(err.to_string())),
    }
    let from_root = if places.inside_target && is_below(root, &item.from) {
        root
    } else {
        places.source
    };
    if !is_below(from_root, &item.from) {
        return skip(index, batch, item, SkipReason::OutsideLocation);
    }
    match has_symlink_ancestor_under(from_root, &item.from) {
        Ok(false) => {}
        Ok(true) => return skip(index, batch, item, SkipReason::LeavesLocation),
        Err(err) => return skip(index, batch, item, SkipReason::Failed(err.to_string())),
    }
    match hash_path(&item.from) {
        Ok(hash) if hash == item.expected_hash => {}
        Ok(_) => return skip(index, batch, item, SkipReason::SourceChanged),
        Err(err) if is_not_found(&err) => {
            return skip(index, batch, item, SkipReason::SourceMissing)
        }
        Err(err) => return skip(index, batch, item, SkipReason::Failed(err.to_string())),
    }
    if transfer == Transfer::Move && is_respelling(&item.from, &item.to) {
        return respell(index, batch, item);
    }
    match fs::symlink_metadata(&item.to) {
        Ok(_) => return skip(index, batch, item, SkipReason::TargetExists),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return skip(index, batch, item, SkipReason::Failed(err.to_string())),
    }

    let seq = index.add_journal_item(batch, &journal_entry(item, JournalState::Pending, None))?;
    if let Err(err) = create_parents(index, batch, root, &item.to) {
        index.set_journal_state(
            batch,
            seq,
            JournalState::Skipped,
            Some(&SkipReason::Failed(err.to_string()).text()),
        )?;
        return Ok(ItemOutcome::Skipped(SkipReason::Failed(err.to_string())));
    }
    let moved = transfer_to(
        &item.from,
        &item.to,
        effect,
        &mut Counting,
        OnConflict::Fail,
    );
    let journaled = JournalItem {
        seq,
        from: item.from.clone(),
        to: item.to.clone(),
        hash: item.expected_hash,
        document_id: item.document_id,
        duplicate: item.duplicate,
        state: JournalState::Pending,
        reason: None,
    };
    let settled = match moved {
        Ok(_) if transfer == Transfer::Copy => settle_copied(index, batch, &journaled)?,
        Ok(_) => settle_moved(index, batch, &journaled)?,
        Err(err) => match settle(index, batch, &journaled) {
            Ok(ItemOutcome::Skipped(SkipReason::Interrupted)) => {
                let reason = SkipReason::Failed(err.to_string());
                index.set_journal_state(batch, seq, JournalState::Skipped, Some(&reason.text()))?;
                ItemOutcome::Skipped(reason)
            }
            Ok(settled) => settled,
            // Left pending: the next `reconcile` looks at the disk again.
            Err(_unsettled) => ItemOutcome::Skipped(SkipReason::Failed(err.to_string())),
        },
    };
    Ok(settled)
}

/// Creates the folders above `to` that are missing, each one recorded so an
/// undo can remove it again.
fn create_parents(
    index: &Index,
    batch: BatchId,
    root: &Path,
    to: &Path,
) -> Result<(), ExecuteError> {
    let Some(parent) = to.parent() else {
        return Ok(());
    };
    let mut missing: Vec<&Path> = parent
        .ancestors()
        .take_while(|ancestor| ancestor.starts_with(root) && *ancestor != root)
        .filter(|ancestor| !ancestor.exists())
        .collect();
    missing.reverse();
    for folder in missing {
        match fs::create_dir(folder) {
            Ok(()) => index.record_created_dir(batch, folder)?,
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(io_error(folder)(err)),
        }
    }
    Ok(())
}

fn stamp_of(path: &Path) -> Result<FileStamp, ExecuteError> {
    let metadata = fs::symlink_metadata(path).map_err(io_error(path))?;
    Ok(FileStamp {
        size: metadata.len(),
        mtime: metadata.modified().map_err(io_error(path))?,
    })
}

/// Points the index at the file's new place. The stamp is the target's: the
/// intake takes a path with the stored size and mtime as unchanged, so the
/// moved file is not taken in a second time.
fn repoint(
    index: &Index,
    document_id: Option<DocumentId>,
    duplicate: bool,
    old: &Path,
    new: &Path,
) -> Result<(), ExecuteError> {
    let Some(id) = document_id else {
        return Ok(());
    };
    let stamp = stamp_of(new)?;
    if duplicate {
        index.remove_duplicate_path(old)?;
        index.record_duplicate_path(id, new, stamp)?;
    } else {
        index.set_document_path(id, new, stamp)?;
    }
    Ok(())
}

fn settle_moved(
    index: &Index,
    batch: BatchId,
    item: &JournalItem,
) -> Result<ItemOutcome, ExecuteError> {
    repoint(
        index,
        item.document_id,
        item.duplicate,
        &item.from,
        &item.to,
    )?;
    if let (Some(BatchKind::Filing | BatchKind::Hermes), Some(id), false) =
        (index.batch_kind(batch)?, item.document_id, item.duplicate)
    {
        // Only what ends up in the folder the run files into counts as
        // filed; a move inside a side archive does not.
        let info = index.batch_info(batch)?;
        let lands_in_target = info
            .as_ref()
            .and_then(|found| found.target_root.as_ref())
            .is_none_or(|root| {
                item.to.starts_with(&root.given) || item.to.starts_with(&root.resolved)
            });
        if lands_in_target && run_steps::is_sorting(info.as_ref()) {
            index.mark_document_filed(id)?;
        } else if lands_in_target {
            index.set_document_status(id, DocumentStatus::Filed)?;
        }
    }
    index.set_journal_state(batch, item.seq, JournalState::Moved, None)?;
    Ok(ItemOutcome::Moved)
}

/// A finished copy: the source stays and the index keeps pointing at it.
fn settle_copied(
    index: &Index,
    batch: BatchId,
    item: &JournalItem,
) -> Result<ItemOutcome, ExecuteError> {
    if run_steps::is_sorting(index.batch_info(batch)?.as_ref()) {
        run_steps::note_copy(index, item)?;
    }
    index.set_journal_state(batch, item.seq, JournalState::Moved, None)?;
    Ok(ItemOutcome::Moved)
}

fn skip_pending(
    index: &Index,
    batch: BatchId,
    item: &JournalItem,
    reason: SkipReason,
) -> Result<ItemOutcome, ExecuteError> {
    index.set_journal_state(batch, item.seq, JournalState::Skipped, Some(&reason.text()))?;
    Ok(ItemOutcome::Skipped(reason))
}

/// Removes what an interrupted copy left under its hidden name next to `to`.
/// Not by process id: a crashed run's id is gone, and a second running instance
/// keeps its own staging fresh, so only age tells a leftover from a live copy.
pub(in crate::execute) fn remove_staging_copies(to: &Path) {
    if let Some(parent) = to.parent() {
        sweep_partials(parent);
    }
}

pub(in crate::execute) fn has_hash(path: &Path, hash: ContentHash) -> bool {
    hash_path(path).is_ok_and(|found| found == hash)
}

/// Decides what became of a `pending` item from what is on disk, and records
/// it, by what the batch does with its files.
fn settle(index: &Index, batch: BatchId, item: &JournalItem) -> Result<ItemOutcome, ExecuteError> {
    match index.batch_transfer(batch)?.unwrap_or(Transfer::Move) {
        Transfer::Move => settle_move(index, batch, item),
        Transfer::Copy => settle_copy(index, batch, item),
        Transfer::Trash => trash::settle_trash(index, batch, item),
    }
}

/// A copy of the content at the target while the source is gone is a
/// finished move; both there with the target intact finishes the move by
/// removing the source, which is the copy that was just verified; the source
/// alone is a move that never happened.
fn settle_move(
    index: &Index,
    batch: BatchId,
    item: &JournalItem,
) -> Result<ItemOutcome, ExecuteError> {
    restore_respelled(item);
    remove_staging_copies(&item.to);
    let source_present = fs::symlink_metadata(&item.from).is_ok();
    let target_intact = has_hash(&item.to, item.hash);
    if target_intact && !source_present {
        return settle_moved(index, batch, item);
    }
    if target_intact && has_hash(&item.from, item.hash) {
        if is_same_entry(&item.from, &item.to) {
            // Another spelling of one file: removing the source removes the target.
            return settle_moved(index, batch, item);
        }
        if !within_recorded_roots(index, batch, item) {
            return skip_pending(
                index,
                batch,
                item,
                SkipReason::Failed("the item lies outside the folders of its run".to_owned()),
            );
        }
        fs::remove_file(&item.from).map_err(io_error(&item.from))?;
        return settle_moved(index, batch, item);
    }
    if !target_intact && fs::symlink_metadata(&item.to).is_err() && has_hash(&item.from, item.hash)
    {
        return skip_pending(index, batch, item, SkipReason::Interrupted);
    }
    skip_pending(
        index,
        batch,
        item,
        SkipReason::Failed("the state on disk is not clear".to_owned()),
    )
}

/// A copy is finished when the target is intact, whatever became of the source,
/// which is never removed. Without an intact target the copy never happened if
/// the source is still whole.
fn settle_copy(
    index: &Index,
    batch: BatchId,
    item: &JournalItem,
) -> Result<ItemOutcome, ExecuteError> {
    remove_staging_copies(&item.to);
    if has_hash(&item.to, item.hash) {
        return settle_copied(index, batch, item);
    }
    if has_hash(&item.from, item.hash) {
        return skip_pending(index, batch, item, SkipReason::Interrupted);
    }
    skip_pending(
        index,
        batch,
        item,
        SkipReason::Failed("the state on disk is not clear".to_owned()),
    )
}

/// Settles what an interrupted run left `pending`, before anything else
/// touches the files. Returns how many items it settled.
pub fn reconcile(index: &Index) -> Result<usize, ExecuteError> {
    let pending = index.pending_journal_items()?;
    let mut batches = Vec::new();
    let mut open_batches = Vec::new();
    let mut settled = 0;
    for (batch, item) in &pending {
        if !batches.contains(batch) {
            batches.push(*batch);
        }
        // An item that cannot be settled now stays `pending`, so the next open
        // looks at the disk again; the others are not held up by it.
        match settle(index, *batch, item) {
            Ok(_) => settled += 1,
            Err(_unsettled) => open_batches.push(*batch),
        }
    }
    for batch in batches
        .into_iter()
        .filter(|batch| !open_batches.contains(batch))
    {
        index.finish_batch(batch)?;
    }
    Ok(settled)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UndoReport {
    pub undone: usize,
    pub left: usize,
}

/// Moves the batch back, last item first. `roots` are the locations the batch
/// may have worked in. An item goes back only when the
/// file at its target still has the planned content and its old place is free;
/// the rest is left and journaled as `undo_skipped`; a later undo tries those
/// again, and the batch counts as undone only when none is left. Then the folders the run
/// created are removed if they are empty, deepest first.
pub fn undo(index: &Index, batch: BatchId, roots: &[PathBuf]) -> Result<UndoReport, ExecuteError> {
    undo_with_trash(index, batch, roots, None)
}

/// `undo`, with the trash a trash batch is put back from. A trash batch
/// without one leaves all its items.
pub fn undo_with_trash(
    index: &Index,
    batch: BatchId,
    roots: &[PathBuf],
    trash: Option<&dyn ncrs_fs::Trash>,
) -> Result<UndoReport, ExecuteError> {
    let recorded = index.batch_info(batch)?;
    let transfer = recorded
        .as_ref()
        .map_or(Transfer::Move, |found| found.transfer);
    let sorting = run_steps::is_sorting(recorded.as_ref());
    let mut all_roots = roots.to_vec();
    if let Some(found) = recorded {
        for recorded_root in [found.source_root, found.target_root].into_iter().flatten() {
            refuse_swapped_root(&recorded_root)?;
            all_roots.push(recorded_root.given);
        }
    }
    let mut report = UndoReport::default();
    let items = index.journal_items(batch)?;
    for item in items
        .iter()
        .rev()
        .filter(|item| matches!(item.state, JournalState::Moved | JournalState::UndoSkipped))
    {
        let undone = match transfer {
            Transfer::Move => undo_item(index, item, &all_roots)?,
            Transfer::Copy => undo_copy_item(index, item, &all_roots)?,
            Transfer::Trash => trash::undo_trash_item(item, trash, &all_roots),
        };
        match undone {
            None => {
                if sorting && transfer == Transfer::Copy {
                    run_steps::forget_copy(index, item)?;
                }
                index.set_journal_state(batch, item.seq, JournalState::Undone, None)?;
                report.undone += 1;
            }
            Some(reason) => {
                index.set_journal_state(
                    batch,
                    item.seq,
                    JournalState::UndoSkipped,
                    Some(&reason.text()),
                )?;
                report.left += 1;
            }
        }
    }
    let mut folders = index.created_dirs(batch)?;
    folders.sort_by_key(|folder| std::cmp::Reverse(folder.components().count()));
    for folder in folders {
        // Fails when the folder is not empty, which is what keeps it.
        let _best_effort = fs::remove_dir(&folder);
    }
    if report.left == 0 {
        index.mark_batch_undone(batch)?;
    }
    Ok(report)
}

/// A copy comes out only while it is still the planned content and the file it
/// was copied from is still there with that content too.
fn undo_copy_item(
    index: &Index,
    item: &JournalItem,
    roots: &[PathBuf],
) -> Result<Option<SkipReason>, ExecuteError> {
    if let Some(reason) = leaves_its_location(roots, item) {
        return Ok(Some(reason));
    }
    match hash_path(&item.to) {
        Ok(hash) if hash == item.hash => {}
        Ok(_) => return Ok(Some(SkipReason::SourceChanged)),
        Err(err) if is_not_found(&err) => return Ok(Some(SkipReason::SourceMissing)),
        Err(err) => return Ok(Some(SkipReason::Failed(err.to_string()))),
    }
    if !has_hash(&item.from, item.hash) {
        return Ok(Some(SkipReason::Failed(
            "the original is gone or changed".to_owned(),
        )));
    }
    if let Err(err) = fs::remove_file(&item.to) {
        return Ok(Some(SkipReason::Failed(err.to_string())));
    }
    ask_again_about_filed_document(index, item)?;
    Ok(None)
}

/// A document whose file is put back is no longer filed. The file stays where
/// it is until someone decides: an open review entry keeps the pipeline from
/// filing it again by itself.
fn ask_again_about_filed_document(index: &Index, item: &JournalItem) -> Result<(), ExecuteError> {
    let (Some(id), false) = (item.document_id, item.duplicate) else {
        return Ok(());
    };
    let filed = index
        .document(id)?
        .is_some_and(|document| document.status == DocumentStatus::Filed);
    if !filed {
        return Ok(());
    }
    // Any other open question would be decided again by itself.
    if let Some(open) = index.open_review_of(id)? {
        index.resolve_review(open.id)?;
    }
    index.add_review_shared(id, ReviewReason::Undone)?;
    Ok(())
}

/// A link between a location and one of the item's two places could lead out
/// of it, to a folder nobody chose.
fn leaves_its_location(roots: &[PathBuf], item: &JournalItem) -> Option<SkipReason> {
    for path in [&item.from, &item.to] {
        for root in roots.iter().filter(|root| path.starts_with(root)) {
            match has_symlink_ancestor_under(root, path) {
                Ok(false) => {}
                Ok(true) => return Some(SkipReason::LeavesLocation),
                Err(err) => return Some(SkipReason::Failed(err.to_string())),
            }
        }
    }
    None
}

/// The check on the ancestors does not look at the root itself. A root that now
/// leads somewhere else than when the run started refuses the whole undo.
fn refuse_swapped_root(source: &SourceRoot) -> Result<(), ExecuteError> {
    match ncrs_intake::canonical_path(&source.given) {
        Ok(now) if now == source.resolved => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Ok(_) | Err(_) => Err(ExecuteError::Io {
            path: source.given.clone(),
            source: io::Error::other("the folder leads somewhere else than when the run started"),
        }),
    }
}

fn undo_item(
    index: &Index,
    item: &JournalItem,
    roots: &[PathBuf],
) -> Result<Option<SkipReason>, ExecuteError> {
    if let Some(reason) = leaves_its_location(roots, item) {
        return Ok(Some(reason));
    }
    match hash_path(&item.to) {
        Ok(hash) if hash == item.hash => {}
        Ok(_) => return Ok(Some(SkipReason::SourceChanged)),
        Err(err) if is_not_found(&err) => return Ok(Some(SkipReason::SourceMissing)),
        Err(err) => return Ok(Some(SkipReason::Failed(err.to_string()))),
    }
    if fs::symlink_metadata(&item.from).is_ok() {
        if !is_respelling(&item.to, &item.from) {
            return Ok(Some(SkipReason::TargetExists));
        }
        if let Err(err) = respell_back(&item.from, &item.to) {
            return Ok(Some(SkipReason::Failed(err.to_string())));
        }
        repoint(
            index,
            item.document_id,
            item.duplicate,
            &item.to,
            &item.from,
        )?;
        ask_again_about_filed_document(index, item)?;
        return Ok(None);
    }
    if let Some(parent) = item.from.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            return Ok(Some(SkipReason::Failed(err.to_string())));
        }
    }
    if let Err(err) = transfer_to(&item.to, &item.from, &Move, &mut Counting, OnConflict::Fail) {
        return Ok(Some(SkipReason::Failed(err.to_string())));
    }
    repoint(
        index,
        item.document_id,
        item.duplicate,
        &item.to,
        &item.from,
    )?;
    ask_again_about_filed_document(index, item)?;
    Ok(None)
}

/// Whether both ends of an item lie in the folders the batch recorded, when it
/// recorded any. A journal that is not the run's own must not make a settle
/// remove files elsewhere.
fn within_recorded_roots(index: &Index, batch: BatchId, item: &JournalItem) -> bool {
    let Ok(Some(info)) = index.batch_info(batch) else {
        return true;
    };
    let lies_in = |root: &SourceRoot, path: &Path| {
        path.starts_with(&root.given) || path.starts_with(&root.resolved)
    };
    let roots: Vec<&SourceRoot> = [&info.source_root, &info.target_root]
        .into_iter()
        .flatten()
        .collect();
    let from_fits = roots.is_empty() || roots.iter().any(|root| lies_in(root, &item.from));
    let to_fits = info
        .target_root
        .as_ref()
        .is_none_or(|root| lies_in(root, &item.to));
    from_fits && to_fits
}

#[cfg(test)]
mod run_safety_tests;
#[cfg(test)]
pub mod test_runs;
#[cfg(test)]
mod tests;
