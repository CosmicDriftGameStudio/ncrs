//! Trash runs: files that "can go" because the same content lies elsewhere.
//! The journal keeps the trash's own key for each entry, so an undo puts back
//! exactly that entry.

use std::path::{Path, PathBuf};

use ncrs_fs::{RestoreError, Trash, TrashReach, TrashedItem};
use ncrs_index::{
    BatchId, BatchKind, ContentHash, Index, JournalItem, JournalState, NewJournalItem, Transfer,
};
use ncrs_intake::{has_symlink_ancestor_under, hash_path};

use super::{
    has_hash, is_below, is_not_found, leaves_its_location, remember_source_root, ExecuteError,
    ItemOutcome, SkipReason,
};

/// A file to put in the trash, and the file that makes it redundant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashItem {
    pub path: PathBuf,
    pub expected_hash: ContentHash,
    /// Stays where it is; must hold the same content.
    pub keeper: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TrashReport {
    pub trashed: usize,
    pub skipped: usize,
}

/// Whether what ncrs trashed can be listed and put back. Without that no
/// trash run is offered.
pub fn trash_can_restore(trash: &dyn Trash) -> bool {
    trash.reach() != TrashReach::FinderOnly
}

fn journal_entry(item: &TrashItem, state: JournalState, reason: Option<String>) -> NewJournalItem {
    NewJournalItem {
        from: item.path.clone(),
        to: item.path.clone(),
        hash: item.expected_hash,
        document_id: None,
        duplicate: false,
        state,
        reason,
    }
}

fn skip(
    index: &Index,
    batch: BatchId,
    item: &TrashItem,
    reason: SkipReason,
) -> Result<(), ExecuteError> {
    index.add_journal_item(
        batch,
        &journal_entry(item, JournalState::Skipped, Some(reason.text())),
    )?;
    Ok(())
}

/// Why this item must not be trashed, if there is a reason.
fn refusal(root: &Path, item: &TrashItem) -> Option<SkipReason> {
    if !is_below(root, &item.path) {
        return Some(SkipReason::OutsideLocation);
    }
    match has_symlink_ancestor_under(root, &item.path) {
        Ok(false) => {}
        Ok(true) => return Some(SkipReason::LeavesLocation),
        Err(err) => return Some(SkipReason::Failed(err.to_string())),
    }
    match hash_path(&item.path) {
        Ok(hash) if hash == item.expected_hash => {}
        Ok(_) => return Some(SkipReason::SourceChanged),
        Err(err) if is_not_found(&err) => return Some(SkipReason::SourceMissing),
        Err(err) => return Some(SkipReason::Failed(err.to_string())),
    }
    (item.keeper == item.path || !has_hash(&item.keeper, item.expected_hash))
        .then(|| SkipReason::Failed("the file that makes it redundant is gone".to_owned()))
}

/// Puts the items in the trash, each only while it still holds the content it
/// was planned with and its keeper still holds it too. Journaled as a
/// `Hermes` batch of the trash kind.
pub fn trash_run(
    index: &Index,
    name: &str,
    root: &Path,
    items: &[TrashItem],
    trash: &dyn Trash,
) -> Result<TrashReport, ExecuteError> {
    let source = remember_source_root(root)?;
    let batch = index.begin_named_batch(BatchKind::Hermes, name, Transfer::Trash)?;
    index.set_batch_source_root(batch, &source)?;
    let mut report = TrashReport::default();
    for item in items {
        if let Some(reason) = refusal(root, item) {
            skip(index, batch, item, reason)?;
            report.skipped += 1;
            continue;
        }
        let seq =
            index.add_journal_item(batch, &journal_entry(item, JournalState::Pending, None))?;
        if let Err(err) = trash.trash(&item.path) {
            let reason = SkipReason::Failed(err);
            index.set_journal_state(batch, seq, JournalState::Skipped, Some(&reason.text()))?;
            report.skipped += 1;
            continue;
        }
        if let Some(key) = newest_entry_of(trash, &item.path) {
            // A key that is not text cannot be journaled; the undo then finds
            // the entry by its old place.
            let _best_effort = index.set_journal_target(batch, seq, &PathBuf::from(key));
        }
        index.set_journal_state(batch, seq, JournalState::Moved, None)?;
        report.trashed += 1;
    }
    index.finish_batch(batch)?;
    Ok(report)
}

/// The entry of the newest deletion of `original`.
fn newest_entry_of(trash: &dyn Trash, original: &Path) -> Option<std::ffi::OsString> {
    newest_listed(&trash.list().ok()?, |entry| entry.original == original)
        .map(|entry| entry.key.clone())
}

fn newest_listed(
    listed: &[TrashedItem],
    wanted: impl Fn(&TrashedItem) -> bool,
) -> Option<&TrashedItem> {
    // The list names the newest first, which wins a tie of the seconds.
    listed
        .iter()
        .rev()
        .filter(|entry| wanted(entry))
        .max_by_key(|entry| entry.deleted_at)
}

/// The file an entry holds, where the trash keeps it where an app can read it:
/// the path itself on macOS, the file beside the `.trashinfo` on Linux. The
/// Windows trash names no such place.
fn trashed_file_of(entry_key: &std::ffi::OsStr) -> Option<PathBuf> {
    let key_path = Path::new(entry_key);
    let held = if key_path
        .extension()
        .is_some_and(|extension| extension == "trashinfo")
    {
        key_path
            .parent()?
            .parent()?
            .join("files")
            .join(key_path.file_stem()?)
    } else {
        key_path.to_path_buf()
    };
    std::fs::symlink_metadata(&held)
        .is_ok_and(|meta| meta.is_file())
        .then_some(held)
}

/// An interrupted trash item is done when the file is gone from its place
/// (the key is then unknown), and never happened when it is still there.
pub fn settle_trash(
    index: &Index,
    batch: BatchId,
    item: &JournalItem,
) -> Result<ItemOutcome, ExecuteError> {
    if std::fs::symlink_metadata(&item.from).is_err() {
        index.set_journal_state(batch, item.seq, JournalState::Moved, None)?;
        return Ok(ItemOutcome::Moved);
    }
    let reason = SkipReason::Interrupted;
    index.set_journal_state(batch, item.seq, JournalState::Skipped, Some(&reason.text()))?;
    Ok(ItemOutcome::Skipped(reason))
}

/// Puts the entry back that the journal recorded. When the key is unknown
/// (`to` is still the old place) the newest entry from that place is taken.
pub fn undo_trash_item(
    item: &JournalItem,
    bin: Option<&dyn Trash>,
    roots: &[PathBuf],
) -> Option<SkipReason> {
    if let Some(reason) = leaves_its_location(roots, item) {
        return Some(reason);
    }
    let Some(trash) = bin else {
        return Some(SkipReason::Failed(
            "no trash to put it back from".to_owned(),
        ));
    };
    let listed = match trash.list() {
        Ok(listed) => listed,
        Err(err) => return Some(SkipReason::Failed(err)),
    };
    let key_known = item.to != item.from;
    let wanted = if key_known {
        listed.iter().find(|entry| entry.key == item.to.as_os_str())
    } else {
        newest_listed(&listed, |entry| entry.original == item.from)
    };
    let Some(found) = wanted else {
        return Some(SkipReason::SourceMissing);
    };
    // A file this run did not trash is never put back in its place. An entry
    // the trash keeps unreadable (Windows) is only trusted under the exact key
    // the journal recorded, never as the newest entry from that place.
    if found.original != item.from {
        return Some(SkipReason::SourceChanged);
    }
    let held_matches = match trashed_file_of(&found.key) {
        Some(held) => has_hash(&held, item.hash),
        None => key_known,
    };
    if !held_matches {
        return Some(SkipReason::SourceChanged);
    }
    match trash.restore(found) {
        Ok(()) => None,
        Err(RestoreError::Taken) => Some(SkipReason::TargetExists),
        Err(RestoreError::NotGranted) => Some(SkipReason::Failed("not granted".to_owned())),
        Err(RestoreError::Failed(reason)) => Some(SkipReason::Failed(reason)),
    }
}
