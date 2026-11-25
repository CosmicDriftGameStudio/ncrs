//! Single steps of a run that is carried out file by file, so the index is
//! free between two files, and what the index learns from them.

use ncrs_fs::transfer::{Copy, Move};
use ncrs_index::{BatchId, BatchInfo, BatchKind, Index, JournalItem, Transfer};

use super::{
    io, remember_source_root, run_item, stamp_of, DocumentStatus, ExecuteError, ExecutionItem,
    ItemOutcome, RunPlaces,
};

/// Opens a batch that records the folders it works between.
pub fn open_run_batch(
    index: &Index,
    kind: BatchKind,
    name: &str,
    transfer: Transfer,
    places: &RunPlaces<'_>,
) -> Result<BatchId, ExecuteError> {
    let source = remember_source_root(places.source)?;
    let batch = index.begin_named_batch(kind, name, transfer)?;
    index.set_batch_source_root(batch, &source)?;
    // A target not made yet has nothing to resolve; the undo then falls back
    // on the places in the journal.
    if let Ok(target) = remember_source_root(places.target) {
        index.set_batch_target_root(batch, &target)?;
    }
    Ok(batch)
}

/// Carries out one item of an open batch; the caller finishes the batch.
pub fn run_one(
    index: &Index,
    batch: BatchId,
    transfer: Transfer,
    places: &RunPlaces<'_>,
    item: &ExecutionItem,
) -> Result<ItemOutcome, ExecuteError> {
    match transfer {
        Transfer::Move => run_item(index, batch, transfer, places, item, &Move),
        Transfer::Copy => run_item(index, batch, transfer, places, item, &Copy),
        Transfer::Trash => Err(ExecuteError::Io {
            path: places.target.to_path_buf(),
            source: io::Error::other("a trash run goes through trash_run"),
        }),
    }
}

/// Only the runs of sorting are grouped.
pub fn is_sorting(info: Option<&BatchInfo>) -> bool {
    info.is_some_and(|found| found.run.is_some())
}

/// The copy is a known place of the document, and the original is not taken in
/// or decided again.
pub fn note_copy(index: &Index, item: &JournalItem) -> Result<(), ExecuteError> {
    let Some(id) = item.document_id else {
        return Ok(());
    };
    index.record_duplicate_path_in(id, &item.to, stamp_of(&item.to)?, true)?;
    if !item.duplicate {
        index.record_filed_inbox_path(id, &item.from, stamp_of(&item.from)?)?;
        index.set_document_status(id, DocumentStatus::Filed)?;
    }
    Ok(())
}

/// The reverse of [`note_copy`] for a copy that was taken out again.
pub fn forget_copy(index: &Index, item: &JournalItem) -> Result<(), ExecuteError> {
    index.remove_duplicate_path(&item.to)?;
    if !item.duplicate {
        index.remove_filed_inbox_path(&item.from)?;
    }
    Ok(())
}
