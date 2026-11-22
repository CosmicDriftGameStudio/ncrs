use std::path::Path;

use rusqlite::{params, OptionalExtension as _};

use crate::documents::require_document;
use crate::error::IndexError;
use crate::index::Index;
use crate::model::{DocumentId, FileStamp};

impl Index {
    /// Remembers that the inbox file at `path` has the content of an already
    /// filed document, as of `stamp`.
    pub fn record_filed_inbox_path(
        &self,
        id: DocumentId,
        path: &Path,
        stamp: FileStamp,
    ) -> Result<(), IndexError> {
        let path_text = path
            .to_str()
            .ok_or_else(|| IndexError::NonUtf8Path(path.to_path_buf()))?;
        require_document(self.connection(), id)?;
        self.connection().execute(
            "INSERT INTO filed_inbox_path (path, document_id, file_size, file_mtime)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (path) DO UPDATE SET
                 document_id = excluded.document_id,
                 file_size = excluded.file_size,
                 file_mtime = excluded.file_mtime",
            params![
                path_text,
                id.get(),
                stamp.size_column(),
                stamp.mtime_nanos()
            ],
        )?;
        Ok(())
    }

    /// The document the file at `path` was found to duplicate, while the file
    /// still has the recorded stamp.
    pub fn filed_document_for_inbox_path(
        &self,
        path: &Path,
        stamp: FileStamp,
    ) -> Result<Option<DocumentId>, IndexError> {
        let path_text = path
            .to_str()
            .ok_or_else(|| IndexError::NonUtf8Path(path.to_path_buf()))?;
        let row: Option<(i64, i64, i64)> = self
            .connection()
            .query_row(
                "SELECT document_id, file_size, file_mtime FROM filed_inbox_path WHERE path = ?1",
                [path_text],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((id, size, mtime)) = row else {
            return Ok(None);
        };
        let recorded = FileStamp::from_columns(Some(size), Some(mtime))?;
        Ok((recorded == Some(stamp)).then(|| DocumentId::new(id)))
    }

    /// Every inbox file noted as a repeat of a filed document, with the
    /// stamp it had then, ordered by path.
    pub fn filed_inbox_paths(&self) -> Result<Vec<FiledInboxCopy>, IndexError> {
        let mut statement = self.connection().prepare(
            "SELECT path, document_id, file_size, file_mtime FROM filed_inbox_path ORDER BY path",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;
        let mut copies = Vec::new();
        for row in rows {
            let (path, id, size, mtime) = row?;
            copies.push(FiledInboxCopy {
                path: path.into(),
                document: DocumentId::new(id),
                stamp: FileStamp::from_columns(Some(size), Some(mtime))?,
            });
        }
        Ok(copies)
    }

    /// The file at `path` is no longer noted as a repeat; `false` when it
    /// was not.
    pub fn remove_filed_inbox_path(&self, path: &Path) -> Result<bool, IndexError> {
        let path_text = path
            .to_str()
            .ok_or_else(|| IndexError::NonUtf8Path(path.to_path_buf()))?;
        let removed = self
            .connection()
            .execute("DELETE FROM filed_inbox_path WHERE path = ?1", [path_text])?;
        Ok(removed > 0)
    }
}

/// An inbox file found to have the content of a filed document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FiledInboxCopy {
    pub path: std::path::PathBuf,
    pub document: DocumentId,
    /// What the file looked like then; `None` when the stored stamp is no
    /// valid one.
    pub stamp: Option<FileStamp>,
}
