//! Carries out a list of moves inside an archive location, with a journal so
//! an interrupted run can be settled and a finished one undone.
//!
//! Nothing here deletes a file the user has: the only thing removed is the
//! source of a move whose copy was checked, and the empty folders a run
//! created itself. A move never overwrites; a source whose content is not the
//! one that was planned is left alone.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ncrs_index::{
    BatchId, BatchKind, ContentHash, DocumentId, DocumentStatus, FileStamp, Index, IndexError,
    JournalItem, JournalState, NewJournalItem,
};
use ncrs_intake::{has_symlink_ancestor_under, hash_path, stays_below, IntakeError};

use ncrs_fs::transfer::{
    partial_prefix, transfer_to, Counting, Move, OnConflict, Progress, Source,
};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemReport {
    pub from: PathBuf,
    pub to: PathBuf,
    pub outcome: ItemOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionReport {
    pub batch: BatchId,
    /// In the order of the input.
    pub items: Vec<ItemReport>,
}

impl ExecutionReport {
    pub fn moved(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.outcome == ItemOutcome::Moved)
            .count()
    }

    pub fn skipped(&self) -> usize {
        self.items.len() - self.moved()
    }
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

/// Moves the items one after the other, inside `root`. Every item gets a
/// journal entry: `pending` before it moves, then `moved`, or `skipped` with
/// the reason when it is not moved. `progress` is told after each item and
/// can stop the run between two items (the rest is journaled as cancelled).
pub fn execute<P: Progress>(
    index: &Index,
    kind: BatchKind,
    root: &Path,
    items: &[ExecutionItem],
    progress: &mut P,
) -> Result<ExecutionReport, ExecuteError> {
    execute_with(index, kind, root, items, progress, &Move)
}

/// `execute` with the effect of the move given, so a test can stand in for a
/// volume boundary or a failing delete.
pub fn execute_with<P: Progress, S: Source>(
    index: &Index,
    kind: BatchKind,
    root: &Path,
    items: &[ExecutionItem],
    progress: &mut P,
    effect: &S,
) -> Result<ExecutionReport, ExecuteError> {
    let batch = index.begin_batch(kind)?;
    let mut reports = Vec::with_capacity(items.len());
    let mut stopped = false;
    for (position, item) in items.iter().enumerate() {
        let outcome = if stopped {
            skip(index, batch, item, SkipReason::Cancelled)?
        } else {
            let outcome = run_item(index, batch, root, item, effect)?;
            stopped = progress.advanced(position + 1, items.len()).is_err();
            outcome
        };
        reports.push(ItemReport {
            from: item.from.clone(),
            to: item.to.clone(),
            outcome,
        });
    }
    index.finish_batch(batch)?;
    Ok(ExecutionReport {
        batch,
        items: reports,
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
    root: &Path,
    item: &ExecutionItem,
    effect: &S,
) -> Result<ItemOutcome, ExecuteError> {
    if !is_below(root, &item.to) {
        return skip(index, batch, item, SkipReason::OutsideLocation);
    }
    match has_symlink_ancestor_under(root, &item.to) {
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
    if let (Some(BatchKind::Filing), Some(id), false) =
        (index.batch_kind(batch)?, item.document_id, item.duplicate)
    {
        index.set_document_status(id, DocumentStatus::Filed)?;
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

fn remove_staging_copies(to: &Path) {
    let (Some(parent), prefix) = (to.parent(), partial_prefix(to)) else {
        return;
    };
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            let path = entry.path();
            let _best_effort = match fs::symlink_metadata(&path) {
                Ok(meta) if meta.is_dir() => fs::remove_dir_all(&path),
                _ => fs::remove_file(&path),
            };
        }
    }
}

fn has_hash(path: &Path, hash: ContentHash) -> bool {
    hash_path(path).is_ok_and(|found| found == hash)
}

/// Decides what became of a `pending` item from what is on disk, and records
/// it. A copy of the content at the target while the source is gone is a
/// finished move; both there with the target intact finishes the move by
/// removing the source, which is the copy that was just verified; the source
/// alone is a move that never happened.
fn settle(index: &Index, batch: BatchId, item: &JournalItem) -> Result<ItemOutcome, ExecuteError> {
    remove_staging_copies(&item.to);
    let source_present = fs::symlink_metadata(&item.from).is_ok();
    let target_intact = has_hash(&item.to, item.hash);
    if target_intact && !source_present {
        return settle_moved(index, batch, item);
    }
    if target_intact && has_hash(&item.from, item.hash) {
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
/// the rest is left and journaled as `undo_skipped`. Then the folders the run
/// created are removed if they are empty, deepest first.
pub fn undo(index: &Index, batch: BatchId, roots: &[PathBuf]) -> Result<UndoReport, ExecuteError> {
    let mut report = UndoReport::default();
    let items = index.journal_items(batch)?;
    for item in items
        .iter()
        .rev()
        .filter(|item| item.state == JournalState::Moved)
    {
        match undo_item(index, item, roots)? {
            None => {
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
    index.mark_batch_undone(batch)?;
    Ok(report)
}

fn undo_item(
    index: &Index,
    item: &JournalItem,
    roots: &[PathBuf],
) -> Result<Option<SkipReason>, ExecuteError> {
    // The folder the file was moved into is the location it belongs to; a
    // link between that root and the old place could lead out of it.
    if let Some(root) = roots.iter().find(|root| item.to.starts_with(root)) {
        match has_symlink_ancestor_under(root, &item.from) {
            Ok(false) => {}
            Ok(true) => return Ok(Some(SkipReason::LeavesLocation)),
            Err(err) => return Ok(Some(SkipReason::Failed(err.to_string()))),
        }
    }
    match hash_path(&item.to) {
        Ok(hash) if hash == item.hash => {}
        Ok(_) => return Ok(Some(SkipReason::SourceChanged)),
        Err(err) if is_not_found(&err) => return Ok(Some(SkipReason::SourceMissing)),
        Err(err) => return Ok(Some(SkipReason::Failed(err.to_string()))),
    }
    if fs::symlink_metadata(&item.from).is_ok() {
        return Ok(Some(SkipReason::TargetExists));
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
    Ok(None)
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
    use ncrs_index::{DocumentSource, NewDocument};
    use tempfile::TempDir;

    use super::*;
    use ncrs_fs::transfer::Counting;

    struct Setup {
        _dir: TempDir,
        root: PathBuf,
        index: Index,
    }

    impl Setup {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("archive");
            fs::create_dir_all(&root).unwrap();
            let index = Index::create_plain(&dir.path().join("index.db")).unwrap();
            Self {
                _dir: dir,
                root,
                index,
            }
        }

        fn write(&self, relative: &str, content: &str) -> PathBuf {
            let path = self.root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, content).unwrap();
            path
        }

        /// A file the index knows as a document.
        fn document(&self, relative: &str, content: &str) -> ExecutionItem {
            let from = self.write(relative, content);
            let hash = hash_path(&from).unwrap();
            let id = self
                .index
                .insert_document(&NewDocument {
                    hash,
                    path: from.clone(),
                    source: DocumentSource::Archive,
                    received_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
                    stamp: Some(stamp_of(&from).unwrap()),
                })
                .unwrap();
            ExecutionItem {
                to: self.root.join("moved").join(relative),
                from,
                expected_hash: hash,
                document_id: Some(id),
                duplicate: false,
            }
        }

        fn run(&self, items: &[ExecutionItem]) -> ExecutionReport {
            execute(
                &self.index,
                BatchKind::Reorganisation,
                &self.root,
                items,
                &mut Counting,
            )
            .unwrap()
        }

        fn states(&self, batch: BatchId) -> Vec<JournalState> {
            self.index
                .journal_items(batch)
                .unwrap()
                .into_iter()
                .map(|item| item.state)
                .collect()
        }
    }

    struct StopAfter(usize);

    impl Progress for StopAfter {
        fn advanced(&mut self, done: usize, _total: usize) -> io::Result<()> {
            if done >= self.0 {
                Err(io::Error::other("stopped"))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn a_moved_document_is_followed_by_the_index_with_the_stamp_of_the_target() {
        let setup = Setup::new();
        let item = setup.document("a.pdf", "alpha");

        let report = setup.run(std::slice::from_ref(&item));

        assert_eq!(report.moved(), 1);
        assert!(!item.from.exists());
        assert_eq!(fs::read_to_string(&item.to).unwrap(), "alpha");
        let document = setup.index.document_by_path(&item.to).unwrap().unwrap();
        assert_eq!(Some(document.id), item.document_id);
        assert_eq!(document.stamp, Some(stamp_of(&item.to).unwrap()));
        assert_eq!(setup.states(report.batch), [JournalState::Moved]);
        assert_eq!(
            setup.index.created_dirs(report.batch).unwrap(),
            [setup.root.join("moved")]
        );
    }

    #[test]
    fn a_duplicate_copy_changes_its_duplicate_path_and_the_keeper_stays() {
        let setup = Setup::new();
        let keeper = setup.document("keeper.pdf", "same");
        let copy_path = setup.write("copy.pdf", "same");
        let id = keeper.document_id.unwrap();
        setup
            .index
            .record_duplicate_path(id, &copy_path, stamp_of(&copy_path).unwrap())
            .unwrap();
        let item = ExecutionItem {
            from: copy_path.clone(),
            to: setup.root.join("dupes/copy.pdf"),
            expected_hash: keeper.expected_hash,
            document_id: Some(id),
            duplicate: true,
        };

        let report = setup.run(std::slice::from_ref(&item));

        assert_eq!(report.moved(), 1);
        assert!(keeper.from.exists());
        assert_eq!(
            setup.index.duplicate_paths(id).unwrap(),
            std::slice::from_ref(&item.to)
        );
        assert_eq!(
            setup
                .index
                .document_by_path(&keeper.from)
                .unwrap()
                .map(|d| d.id),
            Some(id)
        );
    }

    #[test]
    fn nothing_is_overwritten_and_nothing_foreign_is_touched() {
        let setup = Setup::new();
        let taken = setup.document("taken.pdf", "one");
        setup.write("moved/taken.pdf", "someone else");
        let changed = setup.document("changed.pdf", "two");
        fs::write(&changed.from, "edited after the plan").unwrap();
        let missing = setup.document("missing.pdf", "three");
        fs::remove_file(&missing.from).unwrap();
        let outside = ExecutionItem {
            to: setup._dir.path().join("elsewhere.pdf"),
            ..setup.document("outside.pdf", "four")
        };

        let report = setup.run(&[taken.clone(), changed.clone(), missing, outside.clone()]);

        let outcomes: Vec<_> = report
            .items
            .iter()
            .map(|item| item.outcome.clone())
            .collect();
        assert_eq!(
            outcomes,
            [
                ItemOutcome::Skipped(SkipReason::TargetExists),
                ItemOutcome::Skipped(SkipReason::SourceChanged),
                ItemOutcome::Skipped(SkipReason::SourceMissing),
                ItemOutcome::Skipped(SkipReason::OutsideLocation),
            ]
        );
        assert_eq!(
            fs::read_to_string(setup.root.join("moved/taken.pdf")).unwrap(),
            "someone else"
        );
        assert_eq!(fs::read_to_string(&taken.from).unwrap(), "one");
        assert_eq!(
            fs::read_to_string(&changed.from).unwrap(),
            "edited after the plan"
        );
        assert!(outside.from.exists());
        assert_eq!(setup.states(report.batch), [JournalState::Skipped; 4]);
    }

    #[test]
    fn a_cancel_between_two_items_journals_the_rest_as_cancelled() {
        let setup = Setup::new();
        let first = setup.document("1.pdf", "one");
        let second = setup.document("2.pdf", "two");
        let third = setup.document("3.pdf", "three");

        let report = execute(
            &setup.index,
            BatchKind::Reorganisation,
            &setup.root,
            &[first.clone(), second.clone(), third.clone()],
            &mut StopAfter(1),
        )
        .unwrap();

        assert_eq!(report.moved(), 1);
        assert!(first.to.exists());
        assert!(second.from.exists() && third.from.exists());
        let items = setup.index.journal_items(report.batch).unwrap();
        assert_eq!(items[1].state, JournalState::Skipped);
        assert_eq!(items[1].reason.as_deref(), Some("cancelled"));
        assert_eq!(items[2].reason.as_deref(), Some("cancelled"));
    }

    #[test]
    fn undo_moves_everything_back_and_removes_the_folders_the_run_created() {
        let setup = Setup::new();
        let first = setup.document("a/1.pdf", "one");
        let second = setup.document("b/2.pdf", "two");
        let report = setup.run(&[first.clone(), second.clone()]);

        let undone = undo(
            &setup.index,
            report.batch,
            std::slice::from_ref(&setup.root),
        )
        .unwrap();

        assert_eq!(undone, UndoReport { undone: 2, left: 0 });
        assert_eq!(fs::read_to_string(&first.from).unwrap(), "one");
        assert_eq!(fs::read_to_string(&second.from).unwrap(), "two");
        assert!(!setup.root.join("moved").exists());
        assert_eq!(
            setup
                .index
                .document_by_path(&first.from)
                .unwrap()
                .map(|d| d.id),
            first.document_id
        );
        assert_eq!(setup.states(report.batch), [JournalState::Undone; 2]);
        assert_eq!(
            setup
                .index
                .last_undoable_batch(BatchKind::Reorganisation)
                .unwrap(),
            None
        );
    }

    #[test]
    fn undo_leaves_a_file_that_changed_or_whose_old_place_is_taken() {
        let setup = Setup::new();
        let edited = setup.document("edited.pdf", "one");
        let blocked = setup.document("blocked.pdf", "two");
        let fine = setup.document("fine.pdf", "three");
        let report = setup.run(&[edited.clone(), blocked.clone(), fine.clone()]);
        fs::write(&edited.to, "edited later").unwrap();
        fs::write(&blocked.from, "new file at the old place").unwrap();
        setup.write("moved/keep.txt", "the user's own file");

        let undone = undo(
            &setup.index,
            report.batch,
            std::slice::from_ref(&setup.root),
        )
        .unwrap();

        assert_eq!(undone, UndoReport { undone: 1, left: 2 });
        assert_eq!(fs::read_to_string(&edited.to).unwrap(), "edited later");
        assert_eq!(fs::read_to_string(&blocked.to).unwrap(), "two");
        assert_eq!(
            fs::read_to_string(&blocked.from).unwrap(),
            "new file at the old place"
        );
        assert!(fine.from.exists());
        assert!(
            setup.root.join("moved").exists(),
            "a folder with content stays"
        );
        assert_eq!(
            setup.states(report.batch),
            [
                JournalState::UndoSkipped,
                JournalState::UndoSkipped,
                JournalState::Undone
            ]
        );
    }

    /// A pending item as an interrupted run leaves it.
    fn pending(setup: &Setup, item: &ExecutionItem) -> (BatchId, u32) {
        let batch = setup.index.begin_batch(BatchKind::Reorganisation).unwrap();
        let seq = setup
            .index
            .add_journal_item(batch, &journal_entry(item, JournalState::Pending, None))
            .unwrap();
        (batch, seq)
    }

    fn state_of(setup: &Setup, batch: BatchId) -> JournalItem {
        setup.index.journal_items(batch).unwrap().remove(0)
    }

    #[test]
    fn reconcile_takes_a_finished_move_into_the_index() {
        let setup = Setup::new();
        let item = setup.document("a.pdf", "alpha");
        let (batch, _) = pending(&setup, &item);
        fs::create_dir_all(item.to.parent().unwrap()).unwrap();
        fs::rename(&item.from, &item.to).unwrap();

        assert_eq!(reconcile(&setup.index).unwrap(), 1);

        assert_eq!(state_of(&setup, batch).state, JournalState::Moved);
        assert_eq!(
            setup
                .index
                .document_by_path(&item.to)
                .unwrap()
                .map(|d| d.id),
            item.document_id
        );
    }

    #[test]
    fn reconcile_marks_a_move_that_never_began_as_interrupted() {
        let setup = Setup::new();
        let item = setup.document("a.pdf", "alpha");
        let (batch, _) = pending(&setup, &item);

        reconcile(&setup.index).unwrap();

        let settled = state_of(&setup, batch);
        assert_eq!(settled.state, JournalState::Skipped);
        assert_eq!(settled.reason.as_deref(), Some("interrupted"));
        assert!(item.from.exists());
    }

    #[test]
    fn reconcile_finishes_a_copy_that_was_made_but_whose_source_was_not_removed() {
        let setup = Setup::new();
        let item = setup.document("a.pdf", "alpha");
        let (batch, _) = pending(&setup, &item);
        fs::create_dir_all(item.to.parent().unwrap()).unwrap();
        fs::copy(&item.from, &item.to).unwrap();
        let staging = item
            .to
            .with_file_name(format!("{}1", partial_prefix(&item.to)));
        fs::write(&staging, "half").unwrap();

        reconcile(&setup.index).unwrap();

        assert_eq!(state_of(&setup, batch).state, JournalState::Moved);
        assert!(!item.from.exists());
        assert!(item.to.exists());
        assert!(!staging.exists(), "the staging copy is removed");
    }

    #[test]
    fn reconcile_does_not_guess_when_both_files_differ() {
        let setup = Setup::new();
        let item = setup.document("a.pdf", "alpha");
        let (batch, _) = pending(&setup, &item);
        fs::create_dir_all(item.to.parent().unwrap()).unwrap();
        fs::write(&item.to, "something else").unwrap();

        reconcile(&setup.index).unwrap();

        assert_eq!(state_of(&setup, batch).state, JournalState::Skipped);
        assert!(item.from.exists());
        assert_eq!(fs::read_to_string(&item.to).unwrap(), "something else");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_folder_that_leads_out_of_the_location_is_not_followed() {
        let setup = Setup::new();
        let outside = setup._dir.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, setup.root.join("link")).unwrap();
        let item = ExecutionItem {
            to: setup.root.join("link/deeper/a.pdf"),
            ..setup.document("a.pdf", "alpha")
        };

        let report = setup.run(std::slice::from_ref(&item));

        assert_eq!(
            report.items[0].outcome,
            ItemOutcome::Skipped(SkipReason::LeavesLocation)
        );
        assert!(item.from.exists());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn undo_does_not_move_a_file_back_through_a_link_folder() {
        let setup = Setup::new();
        let item = setup.document("a/a.pdf", "alpha");
        let report = setup.run(std::slice::from_ref(&item));
        let outside = setup._dir.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::remove_dir(item.from.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&outside, item.from.parent().unwrap()).unwrap();

        let undone = undo(
            &setup.index,
            report.batch,
            std::slice::from_ref(&setup.root),
        )
        .unwrap();

        assert_eq!(undone, UndoReport { undone: 0, left: 1 });
        assert!(item.to.exists());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn reconcile_goes_on_after_an_item_it_cannot_settle() {
        use std::os::unix::fs::PermissionsExt as _;

        let setup = Setup::new();
        let stuck = setup.document("locked/a.pdf", "alpha");
        let (stuck_batch, _) = pending(&setup, &stuck);
        fs::create_dir_all(stuck.to.parent().unwrap()).unwrap();
        fs::copy(&stuck.from, &stuck.to).unwrap();
        let locked = stuck.from.parent().unwrap().to_path_buf();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
        let fine = setup.document("b.pdf", "beta");
        let (fine_batch, _) = pending(&setup, &fine);
        fs::create_dir_all(fine.to.parent().unwrap()).unwrap();
        fs::rename(&fine.from, &fine.to).unwrap();

        let settled = reconcile(&setup.index);
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

        assert_eq!(settled.unwrap(), 1);
        assert_eq!(state_of(&setup, fine_batch).state, JournalState::Moved);
        assert_eq!(state_of(&setup, stuck_batch).state, JournalState::Pending);
    }
}
