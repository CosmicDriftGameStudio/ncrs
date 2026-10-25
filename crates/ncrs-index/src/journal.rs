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
}

impl BatchKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reorganisation => "reorganisation",
            Self::Filing => "filing",
        }
    }

    fn parse(value: &str) -> Result<Self, IndexError> {
        match value {
            "reorganisation" => Ok(Self::Reorganisation),
            "filing" => Ok(Self::Filing),
            other => Err(corrupt("execution_batch.kind", other)),
        }
    }
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
        self.connection().execute(
            "INSERT INTO execution_batch (kind, started_at) VALUES (?1, ?2)",
            params![kind.as_str(), Utc::now().timestamp()],
        )?;
        Ok(BatchId::new(self.connection().last_insert_rowid()))
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
        let found: Option<i64> = self
            .connection()
            .query_row(
                "SELECT id FROM execution_batch b
                 WHERE kind = ?1 AND undone_at IS NULL
                     AND EXISTS (SELECT 1 FROM execution_item i
                                 WHERE i.batch_id = b.id AND i.state = 'moved')
                 ORDER BY id DESC LIMIT 1",
                [kind.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        Ok(found.map(BatchId::new))
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
