//! Renaming a file to another spelling of its own name.

use std::fs;
use std::path::{Path, PathBuf};

use ncrs_index::{BatchId, Index, JournalItem, JournalState};

use super::{
    has_hash, journal_entry, settle_moved, skip_pending, ExecuteError, ExecutionItem, ItemOutcome,
    SkipReason,
};

/// The same file under another spelling of its name, in the same folder: on a
/// case-insensitive file system `Rechnung.pdf` and `rechnung.pdf` are one entry.
/// Two names of one file (links) are two entries and do not count.
pub fn is_respelling(from: &Path, to: &Path) -> bool {
    let (Some(from_parent), Some(to_parent), Some(name)) =
        (from.parent(), to.parent(), to.file_name())
    else {
        return false;
    };
    let same_folder = same_file::is_same_file(from_parent, to_parent).unwrap_or(false);
    let same_entry = same_file::is_same_file(from, to).unwrap_or(false);
    let to_is_listed = fs::read_dir(to_parent)
        .map(|entries| entries.flatten().any(|entry| entry.file_name() == name))
        .unwrap_or(true);
    same_folder && same_entry && !to_is_listed
}

/// Whether both paths lead to one file, whatever their spelling.
pub(in crate::execute) fn is_same_entry(from: &Path, to: &Path) -> bool {
    same_file::is_same_file(from, to).unwrap_or(false)
}

/// Gives a file that now has `current` its old spelling `original`.
pub(in crate::execute) fn respell_back(original: &Path, current: &Path) -> std::io::Result<()> {
    let hidden = respell_hidden_name(current);
    fs::rename(current, &hidden)?;
    fs::rename(&hidden, original).inspect_err(|_| {
        let _put_back = fs::rename(&hidden, current);
    })
}

fn respell_hidden_name(to: &Path) -> PathBuf {
    let name = to.file_name().unwrap_or_default().to_string_lossy();
    to.with_file_name(format!(".{name}.ncrs-respell"))
}

/// A respell that stopped between its two renames left the file only under
/// the hidden name: it goes to its new name when neither name is there.
pub(in crate::execute) fn restore_respelled(item: &JournalItem) {
    let hidden = respell_hidden_name(&item.to);
    let nothing_at_either_name =
        fs::symlink_metadata(&item.from).is_err() && fs::symlink_metadata(&item.to).is_err();
    if nothing_at_either_name && has_hash(&hidden, item.hash) {
        let _best_effort = fs::rename(&hidden, &item.to);
    }
}

/// Renames through a hidden name, which also works where the old and the new
/// name are one entry for the file system.
pub(in crate::execute) fn respell(
    index: &Index,
    batch: BatchId,
    item: &ExecutionItem,
) -> Result<ItemOutcome, ExecuteError> {
    let seq = index.add_journal_item(batch, &journal_entry(item, JournalState::Pending, None))?;
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
    let hidden = respell_hidden_name(&item.to);
    let renamed = fs::rename(&item.from, &hidden).and_then(|()| {
        fs::rename(&hidden, &item.to).inspect_err(|_| {
            let _put_back = fs::rename(&hidden, &item.from);
        })
    });
    match renamed {
        Ok(()) => settle_moved(index, batch, &journaled),
        Err(err) => skip_pending(
            index,
            batch,
            &journaled,
            SkipReason::Failed(err.to_string()),
        ),
    }
}
