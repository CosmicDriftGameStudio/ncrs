//! The journal of moves the app made in the archive: what was moved from
//! where to where, so it can be reconciled after an interruption and undone.

use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::{params, OptionalExtension as _};

use crate::error::IndexError;
use crate::index::Index;
use crate::model::{corrupt, hash_from_column, BatchId, ContentHash, DocumentId};

fn utf8(path: &Path) -> Result<&str, IndexError> {
    path.to_str()
        .ok_or_else(|| IndexError::NonUtf8Path(path.to_path_buf()))
}

/// What started a batch of moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchKind {
    Reorganisation,
    Filing,
    /// A run the chat started.
    Hermes,
}

impl BatchKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reorganisation => "reorganisation",
            Self::Filing => "filing",
            Self::Hermes => "hermes",
        }
    }

    fn parse(value: &str) -> Result<Self, IndexError> {
        match value {
            "reorganisation" => Ok(Self::Reorganisation),
            "filing" => Ok(Self::Filing),
            "hermes" => Ok(Self::Hermes),
            other => Err(corrupt("execution_batch.kind", other)),
        }
    }
}

/// What a batch does with its files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transfer {
    Move,
    Copy,
    Trash,
}

impl Transfer {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Move => "move",
            Self::Copy => "copy",
            Self::Trash => "trash",
        }
    }

    fn parse(value: &str) -> Result<Self, IndexError> {
        match value {
            "move" => Ok(Self::Move),
            "copy" => Ok(Self::Copy),
            "trash" => Ok(Self::Trash),
            other => Err(corrupt("execution_batch.transfer", other)),
        }
    }
}

/// What the journal knows about a batch as a whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchInfo {
    pub kind: BatchKind,
    pub name: Option<String>,
    pub transfer: Transfer,
    /// Unix seconds.
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub undone_at: Option<i64>,
    pub source_root: Option<SourceRoot>,
    /// `None` for a run that did not record one.
    pub target_root: Option<SourceRoot>,
}

/// The folder a run took its files from, or put them in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRoot {
    /// The spelling the journal's paths start with.
    pub given: PathBuf,
    /// The same folder with its links resolved when the run started.
    pub resolved: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalState {
    /// Written before the move starts. Still this after a crash.
    Pending,
    Moved,
    /// Not moved, with the reason.
    Skipped,
    /// Moved back by an undo.
    Undone,
    /// A moved item the undo left where it is, with the reason.
    UndoSkipped,
}

impl JournalState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Moved => "moved",
            Self::Skipped => "skipped",
            Self::Undone => "undone",
            Self::UndoSkipped => "undo_skipped",
        }
    }

    fn parse(value: &str) -> Result<Self, IndexError> {
        match value {
            "pending" => Ok(Self::Pending),
            "moved" => Ok(Self::Moved),
            "skipped" => Ok(Self::Skipped),
            "undone" => Ok(Self::Undone),
            "undo_skipped" => Ok(Self::UndoSkipped),
            other => Err(corrupt("execution_item.state", other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewJournalItem {
    pub from: PathBuf,
    pub to: PathBuf,
    /// The hash the file at `from` was expected to have.
    pub hash: ContentHash,
    pub document_id: Option<DocumentId>,
    /// A copy recorded in `duplicate_path`, not the document's own file.
    pub duplicate: bool,
    pub state: JournalState,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalItem {
    pub seq: u32,
    pub from: PathBuf,
    pub to: PathBuf,
    pub hash: ContentHash,
    pub document_id: Option<DocumentId>,
    pub duplicate: bool,
    pub state: JournalState,
    pub reason: Option<String>,
}

type Row = (
    u32,
    String,
    String,
    Vec<u8>,
    Option<i64>,
    bool,
    String,
    Option<String>,
);

fn item_of(row: Row) -> Result<JournalItem, IndexError> {
    let (seq, from, to, hash, document_id, duplicate, state, reason) = row;
    Ok(JournalItem {
        seq,
        from: PathBuf::from(from),
        to: PathBuf::from(to),
        hash: hash_from_column(&hash)?,
        document_id: document_id.map(DocumentId::new),
        duplicate,
        state: JournalState::parse(&state)?,
        reason,
    })
}

const ITEM_COLUMNS: &str =
    "seq, from_path, to_path, hash, document_id, duplicate, state, reason FROM execution_item";

fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Row> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
    ))
}

impl Index {
    pub fn begin_batch(&self, kind: BatchKind) -> Result<BatchId, IndexError> {
        self.insert_batch(kind, None, Transfer::Move)
    }

    pub fn begin_named_batch(
        &self,
        kind: BatchKind,
        name: &str,
        transfer: Transfer,
    ) -> Result<BatchId, IndexError> {
        self.insert_batch(kind, Some(name), transfer)
    }

    fn insert_batch(
        &self,
        kind: BatchKind,
        name: Option<&str>,
        transfer: Transfer,
    ) -> Result<BatchId, IndexError> {
        self.connection().execute(
            "INSERT INTO execution_batch (kind, started_at, name, transfer)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                kind.as_str(),
                Utc::now().timestamp(),
                name,
                transfer.as_str()
            ],
        )?;
        Ok(BatchId::new(self.connection().last_insert_rowid()))
    }

    pub fn batch_info(&self, batch: BatchId) -> Result<Option<BatchInfo>, IndexError> {
        let found = self
            .connection()
            .query_row(
                "SELECT kind, name, transfer, started_at, finished_at, undone_at,
                        source_root, source_resolved, target_root, target_resolved
                 FROM execution_batch WHERE id = ?1",
                [batch.get()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                        row.get::<_, Option<i64>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, Option<String>>(7)?,
                        row.get::<_, Option<String>>(8)?,
                        row.get::<_, Option<String>>(9)?,
                    ))
                },
            )
            .optional()?;
        found
            .map(
                |(
                    kind,
                    name,
                    transfer,
                    started_at,
                    finished_at,
                    undone_at,
                    source_root,
                    source_resolved,
                    target_root,
                    target_resolved,
                )| {
                    let root_of = |written: Option<String>, canonical: Option<String>| {
                        written.zip(canonical).map(|(given, resolved)| SourceRoot {
                            given: PathBuf::from(given),
                            resolved: PathBuf::from(resolved),
                        })
                    };
                    Ok(BatchInfo {
                        kind: BatchKind::parse(&kind)?,
                        name,
                        transfer: Transfer::parse(&transfer)?,
                        started_at,
                        finished_at,
                        undone_at,
                        source_root: root_of(source_root, source_resolved),
                        target_root: root_of(target_root, target_resolved),
                    })
                },
            )
            .transpose()
    }

    /// Records the folder the run takes its files from.
    pub fn set_batch_source_root(
        &self,
        batch: BatchId,
        source: &SourceRoot,
    ) -> Result<(), IndexError> {
        self.connection().execute(
            "UPDATE execution_batch SET source_root = ?2, source_resolved = ?3 WHERE id = ?1",
            params![batch.get(), utf8(&source.given)?, utf8(&source.resolved)?],
        )?;
        Ok(())
    }

    /// Records the folder the run puts its files in.
    pub fn set_batch_target_root(
        &self,
        batch: BatchId,
        target: &SourceRoot,
    ) -> Result<(), IndexError> {
        self.connection().execute(
            "UPDATE execution_batch SET target_root = ?2, target_resolved = ?3 WHERE id = ?1",
            params![batch.get(), utf8(&target.given)?, utf8(&target.resolved)?],
        )?;
        Ok(())
    }

    pub fn batch_transfer(&self, batch: BatchId) -> Result<Option<Transfer>, IndexError> {
        Ok(self.batch_info(batch)?.map(|info| info.transfer))
    }

    /// Appends an item to the batch and returns its position.
    pub fn add_journal_item(
        &self,
        batch: BatchId,
        item: &NewJournalItem,
    ) -> Result<u32, IndexError> {
        let seq: u32 = self.connection().query_row(
            "SELECT coalesce(max(seq) + 1, 0) FROM execution_item WHERE batch_id = ?1",
            [batch.get()],
            |row| row.get(0),
        )?;
        self.connection().execute(
            "INSERT INTO execution_item
                 (batch_id, seq, from_path, to_path, hash, document_id, duplicate, state, reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                batch.get(),
                seq,
                utf8(&item.from)?,
                utf8(&item.to)?,
                item.hash.0.as_slice(),
                item.document_id.map(DocumentId::get),
                item.duplicate,
                item.state.as_str(),
                item.reason
            ],
        )?;
        Ok(seq)
    }

    pub fn set_journal_state(
        &self,
        batch: BatchId,
        seq: u32,
        state: JournalState,
        reason: Option<&str>,
    ) -> Result<(), IndexError> {
        self.connection().execute(
            "UPDATE execution_item SET state = ?3, reason = ?4
             WHERE batch_id = ?1 AND seq = ?2",
            params![batch.get(), seq, state.as_str(), reason],
        )?;
        Ok(())
    }

    /// Replaces where the item went; a trash batch records the trash's own key.
    pub fn set_journal_target(
        &self,
        batch: BatchId,
        seq: u32,
        to: &Path,
    ) -> Result<(), IndexError> {
        self.connection().execute(
            "UPDATE execution_item SET to_path = ?3 WHERE batch_id = ?1 AND seq = ?2",
            params![batch.get(), seq, utf8(to)?],
        )?;
        Ok(())
    }

    /// A folder the batch had to create, so an undo can remove it again.
    pub fn record_created_dir(&self, batch: BatchId, path: &Path) -> Result<(), IndexError> {
        self.connection().execute(
            "INSERT OR IGNORE INTO execution_created_dir (batch_id, path) VALUES (?1, ?2)",
            params![batch.get(), utf8(path)?],
        )?;
        Ok(())
    }

    pub fn finish_batch(&self, batch: BatchId) -> Result<(), IndexError> {
        self.connection().execute(
            "UPDATE execution_batch SET finished_at = ?2 WHERE id = ?1",
            params![batch.get(), Utc::now().timestamp()],
        )?;
        Ok(())
    }

    pub fn mark_batch_undone(&self, batch: BatchId) -> Result<(), IndexError> {
        self.connection().execute(
            "UPDATE execution_batch SET undone_at = ?2 WHERE id = ?1",
            params![batch.get(), Utc::now().timestamp()],
        )?;
        Ok(())
    }

    /// In the order the items were added.
    pub fn journal_items(&self, batch: BatchId) -> Result<Vec<JournalItem>, IndexError> {
        let mut statement = self.connection().prepare(&format!(
            "SELECT {ITEM_COLUMNS} WHERE batch_id = ?1 ORDER BY seq"
        ))?;
        let rows = statement
            .query_map([batch.get()], read_row)?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter().map(item_of).collect()
    }

    /// Sorted by path.
    pub fn created_dirs(&self, batch: BatchId) -> Result<Vec<PathBuf>, IndexError> {
        let mut statement = self
            .connection()
            .prepare("SELECT path FROM execution_created_dir WHERE batch_id = ?1 ORDER BY path")?;
        let paths = statement
            .query_map([batch.get()], |row| {
                row.get::<_, String>(0).map(PathBuf::from)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(paths)
    }

    /// The latest batch of this kind that was not undone and moved something.
    pub fn last_undoable_batch(&self, kind: BatchKind) -> Result<Option<BatchId>, IndexError> {
        Ok(self.undoable_batches(kind, 1)?.into_iter().next())
    }

    /// The latest batches of this kind that were not undone and moved
    /// something, newest first.
    pub fn undoable_batches(
        &self,
        kind: BatchKind,
        limit: usize,
    ) -> Result<Vec<BatchId>, IndexError> {
        let mut statement = self.connection().prepare(
            "SELECT id FROM execution_batch b
             WHERE kind = ?1 AND undone_at IS NULL
                 AND EXISTS (SELECT 1 FROM execution_item i
                             WHERE i.batch_id = b.id AND i.state = 'moved')
             ORDER BY id DESC LIMIT ?2",
        )?;
        let found = statement
            .query_map(
                params![kind.as_str(), i64::try_from(limit).unwrap_or(i64::MAX)],
                |row| row.get::<_, i64>(0),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(found.into_iter().map(BatchId::new).collect())
    }

    pub fn batch_kind(&self, batch: BatchId) -> Result<Option<BatchKind>, IndexError> {
        let found: Option<String> = self
            .connection()
            .query_row(
                "SELECT kind FROM execution_batch WHERE id = ?1",
                [batch.get()],
                |row| row.get(0),
            )
            .optional()?;
        found.as_deref().map(BatchKind::parse).transpose()
    }

    /// Items an interrupted run left `pending`, by batch and position.
    pub fn pending_journal_items(&self) -> Result<Vec<(BatchId, JournalItem)>, IndexError> {
        let mut statement = self.connection().prepare(
            "SELECT batch_id, seq, from_path, to_path, hash, document_id, duplicate, state, reason
             FROM execution_item WHERE state = 'pending' ORDER BY batch_id, seq",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    (
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                    ),
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(batch, row)| Ok((BatchId::new(batch), item_of(row)?)))
            .collect()
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::test_support::encrypted_index;

    fn pending(from: &str, to: &str, seed: u8) -> NewJournalItem {
        NewJournalItem {
            from: PathBuf::from(from),
            to: PathBuf::from(to),
            hash: ContentHash([seed; 32]),
            document_id: None,
            duplicate: false,
            state: JournalState::Pending,
            reason: None,
        }
    }

    #[test]
    fn items_are_kept_in_order_with_their_state_and_reason() {
        let (_dir, index) = encrypted_index();
        let batch = index.begin_batch(BatchKind::Reorganisation).unwrap();
        let first = index
            .add_journal_item(batch, &pending("/a/x", "/a/y", 1))
            .unwrap();
        let second = index
            .add_journal_item(batch, &pending("/a/p", "/a/q", 2))
            .unwrap();
        index
            .set_journal_state(batch, first, JournalState::Moved, None)
            .unwrap();
        index
            .set_journal_state(batch, second, JournalState::Skipped, Some("changed"))
            .unwrap();

        let items = index.journal_items(batch).unwrap();

        assert_eq!((items[0].seq, items[1].seq), (0, 1));
        assert_eq!(items[0].state, JournalState::Moved);
        assert_eq!(items[1].state, JournalState::Skipped);
        assert_eq!(items[1].reason.as_deref(), Some("changed"));
        assert_eq!(items[1].hash, ContentHash([2; 32]));
    }

    #[test]
    fn only_a_batch_that_moved_something_and_was_not_undone_is_undoable() {
        let (_dir, index) = encrypted_index();
        let empty = index.begin_batch(BatchKind::Reorganisation).unwrap();
        index
            .add_journal_item(
                empty,
                &NewJournalItem {
                    state: JournalState::Skipped,
                    ..pending("/a", "/b", 1)
                },
            )
            .unwrap();
        let first = index.begin_batch(BatchKind::Reorganisation).unwrap();
        let first_seq = index
            .add_journal_item(first, &pending("/a", "/b", 1))
            .unwrap();
        index
            .set_journal_state(first, first_seq, JournalState::Moved, None)
            .unwrap();
        let other_kind = index.begin_batch(BatchKind::Filing).unwrap();
        let other_seq = index
            .add_journal_item(other_kind, &pending("/c", "/d", 1))
            .unwrap();
        index
            .set_journal_state(other_kind, other_seq, JournalState::Moved, None)
            .unwrap();

        assert_eq!(
            index
                .last_undoable_batch(BatchKind::Reorganisation)
                .unwrap(),
            Some(first)
        );

        index.mark_batch_undone(first).unwrap();

        assert_eq!(
            index
                .last_undoable_batch(BatchKind::Reorganisation)
                .unwrap(),
            None
        );
        assert_eq!(
            index.last_undoable_batch(BatchKind::Filing).unwrap(),
            Some(other_kind)
        );
    }

    #[test]
    fn name_and_transfer_round_trip_and_a_plain_batch_reads_back_as_a_move() {
        let (_dir, index) = encrypted_index();
        let named = index
            .begin_named_batch(BatchKind::Hermes, "Tidy up Downloads", Transfer::Copy)
            .unwrap();
        let plain = index.begin_batch(BatchKind::Filing).unwrap();
        let seq = index
            .add_journal_item(named, &pending("/a/x", "/b/x", 1))
            .unwrap();
        index
            .set_journal_state(named, seq, JournalState::Moved, None)
            .unwrap();
        index.finish_batch(named).unwrap();

        let info = index.batch_info(named).unwrap().unwrap();
        assert_eq!(
            (info.kind, info.name.as_deref(), info.transfer),
            (BatchKind::Hermes, Some("Tidy up Downloads"), Transfer::Copy)
        );
        assert!(info.finished_at.is_some() && info.undone_at.is_none());
        let plain_info = index.batch_info(plain).unwrap().unwrap();
        assert_eq!(
            (plain_info.name, plain_info.transfer),
            (None, Transfer::Move)
        );
        assert_eq!(index.batch_transfer(named).unwrap(), Some(Transfer::Copy));
        assert_eq!(index.batch_info(BatchId::new(999)).unwrap(), None);
        assert_eq!(
            index.last_undoable_batch(BatchKind::Hermes).unwrap(),
            Some(named)
        );
    }

    #[test]
    fn pending_items_and_created_folders_are_listed() {
        let (_dir, index) = encrypted_index();
        let batch = index.begin_batch(BatchKind::Reorganisation).unwrap();
        index
            .add_journal_item(batch, &pending("/a/x", "/a/n/y", 1))
            .unwrap();
        let done = index
            .add_journal_item(batch, &pending("/a/p", "/a/q", 2))
            .unwrap();
        index
            .set_journal_state(batch, done, JournalState::Moved, None)
            .unwrap();
        index.record_created_dir(batch, Path::new("/a/n")).unwrap();
        index.record_created_dir(batch, Path::new("/a/n")).unwrap();

        let pending_items = index.pending_journal_items().unwrap();

        assert_eq!(pending_items.len(), 1);
        assert_eq!(pending_items[0].0, batch);
        assert_eq!(pending_items[0].1.to, Path::new("/a/n/y"));
        assert_eq!(index.created_dirs(batch).unwrap(), [PathBuf::from("/a/n")]);
        assert_eq!(
            index.batch_kind(batch).unwrap(),
            Some(BatchKind::Reorganisation)
        );
    }
}
