use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension as _};

use crate::documents::require_document;
use crate::error::IndexError;
use crate::index::Index;
use crate::model::{timestamp_from_column, ContentHash, DocumentId, FileStamp};

fn utf8(path: &Path) -> Result<&str, IndexError> {
    path.to_str()
        .ok_or_else(|| IndexError::NonUtf8Path(path.to_path_buf()))
}

impl Index {
    /// Points the document at the file's new place after it was moved or
    /// renamed. A missing document is found again by this.
    pub fn set_document_path(
        &self,
        id: DocumentId,
        path: &Path,
        stamp: FileStamp,
    ) -> Result<(), IndexError> {
        let changed = self.connection().execute(
            "UPDATE document SET path = ?2, file_size = ?3, file_mtime = ?4, missing_since = NULL
             WHERE id = ?1",
            params![
                id.get(),
                utf8(path)?,
                stamp.size_column(),
                stamp.mtime_nanos()
            ],
        )?;
        if changed == 0 {
            return Err(IndexError::DocumentNotFound(id));
        }
        Ok(())
    }

    /// The file is gone, or no longer has the document's content. Remembers
    /// the first time it was noticed.
    pub fn mark_document_missing(&self, id: DocumentId) -> Result<(), IndexError> {
        let changed = self.connection().execute(
            "UPDATE document SET missing_since = coalesce(missing_since, ?2) WHERE id = ?1",
            params![id.get(), Utc::now().timestamp()],
        )?;
        if changed == 0 {
            return Err(IndexError::DocumentNotFound(id));
        }
        Ok(())
    }

    /// Documents that are not missing and whose path lies in `directory`,
    /// compared by path components. Sorted by path.
    pub fn documents_under(
        &self,
        directory: &Path,
    ) -> Result<Vec<(DocumentId, PathBuf)>, IndexError> {
        let prefix = utf8(directory)?;
        let mut statement = self.connection().prepare(
            "SELECT id, path FROM document
             WHERE missing_since IS NULL AND substr(path, 1, length(?1)) = ?1",
        )?;
        let mut found = statement
            .query_map([prefix], |row| {
                Ok((
                    DocumentId::new(row.get(0)?),
                    PathBuf::from(row.get::<_, String>(1)?),
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        found.retain(|(_, path)| path.starts_with(directory));
        found.sort_by(|left, right| left.1.cmp(&right.1));
        Ok(found)
    }

    /// The file at the document's path changed in place. The hash is replaced
    /// and the full text dropped, because it belongs to the old content; the
    /// fields stay. A hash that another document has is `DuplicateHash` with
    /// that document's id, and nothing changes.
    pub fn replace_document_content(
        &mut self,
        id: DocumentId,
        hash: &ContentHash,
        stamp: FileStamp,
    ) -> Result<(), IndexError> {
        let tx = self.conn_mut().transaction()?;
        require_document(&tx, id)?;
        let holder_id: Option<i64> = tx
            .query_row(
                "SELECT id FROM document WHERE hash = ?1 AND id != ?2",
                params![hash.0.as_slice(), id.get()],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(holder) = holder_id {
            return Err(IndexError::DuplicateHash(DocumentId::new(holder)));
        }
        tx.execute(
            "UPDATE document SET hash = ?2, file_size = ?3, file_mtime = ?4 WHERE id = ?1",
            params![
                id.get(),
                hash.0.as_slice(),
                stamp.size_column(),
                stamp.mtime_nanos()
            ],
        )?;
        tx.execute("DELETE FROM document_text WHERE rowid = ?1", [id.get()])?;
        tx.commit()?;
        Ok(())
    }

    /// Records a second place with the same content. Recording the same path
    /// again only refreshes its stamp; a path that was a copy of another
    /// document moves to this one.
    pub fn record_duplicate_path(
        &self,
        id: DocumentId,
        path: &Path,
        stamp: FileStamp,
    ) -> Result<(), IndexError> {
        let path_text = utf8(path)?;
        require_document(self.connection(), id)?;
        self.connection().execute(
            "INSERT INTO duplicate_path (document_id, path, seen_at, file_size, file_mtime)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (path) DO UPDATE SET
                 seen_at = CASE WHEN document_id = excluded.document_id
                     THEN seen_at ELSE excluded.seen_at END,
                 document_id = excluded.document_id,
                 file_size = excluded.file_size,
                 file_mtime = excluded.file_mtime",
            params![
                id.get(),
                path_text,
                Utc::now().timestamp(),
                stamp.size_column(),
                stamp.mtime_nanos()
            ],
        )?;
        Ok(())
    }

    pub fn document_by_duplicate_path(
        &self,
        path: &Path,
    ) -> Result<Option<(DocumentId, Option<FileStamp>)>, IndexError> {
        let row: Option<(i64, Option<i64>, Option<i64>)> = self
            .connection()
            .query_row(
                "SELECT document_id, file_size, file_mtime FROM duplicate_path WHERE path = ?1",
                [utf8(path)?],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        row.map(|(id, size, mtime)| {
            Ok((DocumentId::new(id), FileStamp::from_columns(size, mtime)?))
        })
        .transpose()
    }

    /// Sorted.
    pub fn duplicate_paths(&self, id: DocumentId) -> Result<Vec<PathBuf>, IndexError> {
        let mut statement = self
            .connection()
            .prepare("SELECT path FROM duplicate_path WHERE document_id = ?1")?;
        let mut paths = statement
            .query_map([id.get()], |row| row.get::<_, String>(0).map(PathBuf::from))?
            .collect::<Result<Vec<_>, _>>()?;
        paths.sort();
        Ok(paths)
    }

    /// The copies of the document with the time each was first seen. Sorted
    /// by path.
    pub fn duplicate_copies(
        &self,
        id: DocumentId,
    ) -> Result<Vec<(PathBuf, DateTime<Utc>)>, IndexError> {
        let mut statement = self
            .connection()
            .prepare("SELECT path, seen_at FROM duplicate_path WHERE document_id = ?1")?;
        let rows = statement
            .query_map([id.get()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut copies = rows
            .into_iter()
            .map(|(path, seen_at)| {
                Ok((
                    PathBuf::from(path),
                    timestamp_from_column("duplicate_path.seen_at", seen_at)?,
                ))
            })
            .collect::<Result<Vec<_>, IndexError>>()?;
        copies.sort();
        Ok(copies)
    }

    /// Copies whose path lies in `directory`, compared by path components:
    /// `/a/b` does not contain `/a/bc/x`. Sorted.
    pub fn duplicate_paths_under(&self, directory: &Path) -> Result<Vec<PathBuf>, IndexError> {
        let prefix = utf8(directory)?;
        let mut statement = self
            .connection()
            .prepare("SELECT path FROM duplicate_path WHERE substr(path, 1, length(?1)) = ?1")?;
        let mut paths = statement
            .query_map([prefix], |row| row.get::<_, String>(0).map(PathBuf::from))?
            .collect::<Result<Vec<_>, _>>()?;
        paths.retain(|path| path.starts_with(directory));
        paths.sort();
        Ok(paths)
    }

    /// Whether the path was recorded.
    pub fn remove_duplicate_path(&self, path: &Path) -> Result<bool, IndexError> {
        let removed = self
            .connection()
            .execute("DELETE FROM duplicate_path WHERE path = ?1", [utf8(path)?])?;
        Ok(removed > 0)
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
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;
    use crate::model::{DocumentFields, DocumentSource, DocumentStatus, FieldOrigin};
    use crate::test_support::{encrypted_index, new_document};

    fn stamp(size: u64, seconds: u64) -> FileStamp {
        FileStamp {
            size,
            mtime: UNIX_EPOCH + Duration::new(seconds, 123_456_789),
        }
    }

    #[test]
    fn a_stamp_and_the_archive_source_round_trip() {
        let (_dir, index) = encrypted_index();
        let mut new = new_document(1);
        new.source = DocumentSource::Archive;
        new.stamp = Some(stamp(42, 1_700_000_000));
        let id = index.insert_document(&new).unwrap();

        let stored = index.document(id).unwrap().unwrap();

        assert_eq!(stored.stamp, new.stamp);
        assert_eq!(stored.source, DocumentSource::Archive);
        assert_eq!(index.document_by_hash(&new.hash).unwrap().unwrap().id, id);
    }

    #[test]
    fn a_stamp_before_the_epoch_round_trips() {
        let (_dir, index) = encrypted_index();
        let before = FileStamp {
            size: 0,
            mtime: UNIX_EPOCH - Duration::new(5, 1),
        };
        let id = index.insert_document(&new_document(1)).unwrap();
        index
            .set_document_path(id, Path::new("a/b.pdf"), before)
            .unwrap();

        assert_eq!(index.document(id).unwrap().unwrap().stamp, Some(before));
    }

    #[test]
    fn document_by_path_finds_only_that_exact_path() {
        let (_dir, index) = encrypted_index();
        let new = new_document(1);
        let id = index.insert_document(&new).unwrap();

        assert_eq!(index.document_by_path(&new.path).unwrap().unwrap().id, id);
        assert!(index
            .document_by_path(Path::new("inbox/scan-2.pdf"))
            .unwrap()
            .is_none());
        assert!(index
            .document_by_path(Path::new("INBOX/scan-1.pdf"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn set_document_path_moves_the_document_and_refreshes_the_stamp() {
        let (_dir, index) = encrypted_index();
        let id = index.insert_document(&new_document(1)).unwrap();

        index
            .set_document_path(id, Path::new("archive/2026/a.pdf"), stamp(7, 10))
            .unwrap();

        let stored = index.document(id).unwrap().unwrap();
        assert_eq!(stored.path, Path::new("archive/2026/a.pdf"));
        assert_eq!(stored.stamp, Some(stamp(7, 10)));
        assert!(index
            .document_by_path(Path::new("inbox/scan-1.pdf"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn set_document_path_reports_an_unknown_document() {
        let (_dir, index) = encrypted_index();
        let missing = DocumentId::new(99);

        let err = index
            .set_document_path(missing, Path::new("a.pdf"), stamp(1, 1))
            .unwrap_err();

        assert!(
            matches!(err, IndexError::DocumentNotFound(id) if id == missing),
            "{err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn paths_that_are_not_utf8_are_refused_everywhere() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt as _;

        let (_dir, mut index) = encrypted_index();
        let id = index.insert_document(&new_document(1)).unwrap();
        let bad = Path::new(OsStr::from_bytes(b"caf\xE9.pdf"));

        let results = [
            index.set_document_path(id, bad, stamp(1, 1)),
            index.record_duplicate_path(id, bad, stamp(1, 1)),
            index.document_by_path(bad).map(|_| ()),
            index.document_by_duplicate_path(bad).map(|_| ()),
            index.duplicate_paths_under(bad).map(|_| ()),
            index.remove_duplicate_path(bad).map(|_| ()),
        ];

        for result in results {
            assert!(
                matches!(result, Err(IndexError::NonUtf8Path(_))),
                "{result:?}"
            );
        }
        index
            .replace_document_content(id, &ContentHash([9; 32]), stamp(1, 1))
            .unwrap();
    }

    #[test]
    fn a_missing_document_is_not_found_by_path_until_it_is_moved_again() {
        let (_dir, index) = encrypted_index();
        let new = new_document(1);
        let id = index.insert_document(&new).unwrap();

        index.mark_document_missing(id).unwrap();

        assert!(index.document_by_path(&new.path).unwrap().is_none());
        assert!(index.document(id).unwrap().unwrap().missing_since.is_some());
        assert!(index
            .documents_under(Path::new("inbox"))
            .unwrap()
            .is_empty());
        assert_eq!(index.document_by_hash(&new.hash).unwrap().unwrap().id, id);

        index.set_document_path(id, &new.path, stamp(1, 1)).unwrap();

        assert!(index.document(id).unwrap().unwrap().missing_since.is_none());
        assert_eq!(index.document_by_path(&new.path).unwrap().unwrap().id, id);
        let err = index
            .mark_document_missing(DocumentId::new(99))
            .unwrap_err();
        assert!(matches!(err, IndexError::DocumentNotFound(_)), "{err}");
    }

    #[test]
    fn documents_under_compares_whole_path_components() {
        let (_dir, index) = encrypted_index();
        let mut ids = Vec::new();
        for (seed, path) in [
            (1, "/a/b/x.pdf"),
            (2, "/a/bc/x.pdf"),
            (3, "/a/b/deeper/y.pdf"),
        ] {
            let id = index.insert_document(&new_document(seed)).unwrap();
            index
                .set_document_path(id, Path::new(path), stamp(1, 1))
                .unwrap();
            ids.push(id);
        }

        let under = index.documents_under(Path::new("/a/b")).unwrap();

        assert_eq!(
            under,
            vec![
                (ids[2], PathBuf::from("/a/b/deeper/y.pdf")),
                (ids[0], PathBuf::from("/a/b/x.pdf"))
            ]
        );
    }

    #[test]
    fn replacing_content_sets_the_hash_and_drops_the_text_but_keeps_the_fields() {
        let (_dir, mut index) = encrypted_index();
        let id = index.insert_document(&new_document(1)).unwrap();
        index.set_document_text(id, "Invoice Company A").unwrap();
        let fields = DocumentFields {
            sender: Some("Company A".to_owned()),
            ..DocumentFields::default()
        };
        index
            .set_document_fields(id, &fields, |_| FieldOrigin::Unknown)
            .unwrap();
        let new_hash = ContentHash([8; 32]);

        index
            .replace_document_content(id, &new_hash, stamp(99, 5))
            .unwrap();

        let stored = index.document(id).unwrap().unwrap();
        assert_eq!(stored.hash, new_hash);
        assert_eq!(stored.stamp, Some(stamp(99, 5)));
        assert_eq!(stored.fields, fields);
        assert!(index.search_text("invoice", 10).unwrap().is_empty());
        assert_eq!(index.document_by_hash(&new_hash).unwrap().unwrap().id, id);
    }

    #[test]
    fn replacing_content_with_a_known_hash_changes_nothing() {
        let (_dir, mut index) = encrypted_index();
        let first = index.insert_document(&new_document(1)).unwrap();
        let second = index.insert_document(&new_document(2)).unwrap();
        index.set_document_text(second, "keep me").unwrap();

        let err = index
            .replace_document_content(second, &new_document(1).hash, stamp(1, 1))
            .unwrap_err();

        assert!(
            matches!(err, IndexError::DuplicateHash(id) if id == first),
            "{err}"
        );
        let stored = index.document(second).unwrap().unwrap();
        assert_eq!(stored.hash, new_document(2).hash);
        assert_eq!(stored.stamp, None);
        assert_eq!(index.search_text("keep", 10).unwrap(), vec![second]);
    }

    #[test]
    fn replacing_content_reports_an_unknown_document() {
        let (_dir, mut index) = encrypted_index();

        let err = index
            .replace_document_content(DocumentId::new(5), &ContentHash([1; 32]), stamp(1, 1))
            .unwrap_err();

        assert!(matches!(err, IndexError::DocumentNotFound(_)), "{err}");
    }

    #[test]
    fn recording_a_copy_twice_keeps_one_row_and_refreshes_the_stamp() {
        let (_dir, index) = encrypted_index();
        let id = index.insert_document(&new_document(1)).unwrap();
        let copy = Path::new("archive/copy.pdf");

        index.record_duplicate_path(id, copy, stamp(1, 1)).unwrap();
        index.record_duplicate_path(id, copy, stamp(2, 2)).unwrap();

        assert_eq!(index.duplicate_paths(id).unwrap(), vec![copy.to_path_buf()]);
        assert_eq!(
            index.document_by_duplicate_path(copy).unwrap(),
            Some((id, Some(stamp(2, 2))))
        );
    }

    #[test]
    fn a_copy_path_recorded_for_another_document_moves_to_it() {
        let (_dir, index) = encrypted_index();
        let first = index.insert_document(&new_document(1)).unwrap();
        let second = index.insert_document(&new_document(2)).unwrap();
        let copy = Path::new("archive/copy.pdf");
        index
            .record_duplicate_path(first, copy, stamp(1, 1))
            .unwrap();

        index
            .record_duplicate_path(second, copy, stamp(1, 1))
            .unwrap();

        assert!(index.duplicate_paths(first).unwrap().is_empty());
        assert_eq!(
            index.duplicate_paths(second).unwrap(),
            vec![copy.to_path_buf()]
        );
    }

    #[test]
    fn recording_a_copy_of_an_unknown_document_is_refused() {
        let (_dir, index) = encrypted_index();

        let err = index
            .record_duplicate_path(DocumentId::new(3), Path::new("a.pdf"), stamp(1, 1))
            .unwrap_err();

        assert!(matches!(err, IndexError::DocumentNotFound(_)), "{err}");
    }

    #[test]
    fn duplicate_paths_are_sorted_and_unknown_ones_are_absent() {
        let (_dir, index) = encrypted_index();
        let id = index.insert_document(&new_document(1)).unwrap();
        for name in ["b.pdf", "c.pdf", "a.pdf"] {
            index
                .record_duplicate_path(id, &Path::new("archive").join(name), stamp(1, 1))
                .unwrap();
        }

        assert_eq!(
            index.duplicate_paths(id).unwrap(),
            ["a.pdf", "b.pdf", "c.pdf"].map(|name| Path::new("archive").join(name))
        );
        assert_eq!(
            index
                .document_by_duplicate_path(Path::new("archive/zzz.pdf"))
                .unwrap(),
            None
        );
    }

    #[test]
    fn duplicate_paths_under_compares_whole_path_components() {
        let (_dir, index) = encrypted_index();
        let id = index.insert_document(&new_document(1)).unwrap();
        for path in [
            "/a/b/x.pdf",
            "/a/b/deeper/y.pdf",
            "/a/bc/x.pdf",
            "/z/b/x.pdf",
        ] {
            index
                .record_duplicate_path(id, Path::new(path), stamp(1, 1))
                .unwrap();
        }

        let under = index.duplicate_paths_under(Path::new("/a/b")).unwrap();

        assert_eq!(
            under,
            vec![
                PathBuf::from("/a/b/deeper/y.pdf"),
                PathBuf::from("/a/b/x.pdf")
            ]
        );
    }

    #[test]
    fn removing_a_copy_reports_whether_it_existed() {
        let (_dir, index) = encrypted_index();
        let id = index.insert_document(&new_document(1)).unwrap();
        let copy = Path::new("archive/copy.pdf");
        index.record_duplicate_path(id, copy, stamp(1, 1)).unwrap();

        assert!(index.remove_duplicate_path(copy).unwrap());
        assert!(!index.remove_duplicate_path(copy).unwrap());
        assert!(index.duplicate_paths(id).unwrap().is_empty());
    }

    #[test]
    fn deleting_a_document_deletes_its_copies() {
        let (_dir, index) = encrypted_index();
        let id = index.insert_document(&new_document(1)).unwrap();
        index
            .record_duplicate_path(id, Path::new("archive/copy.pdf"), stamp(1, 1))
            .unwrap();

        index
            .connection()
            .execute("DELETE FROM document WHERE id = ?1", [id.get()])
            .unwrap();

        assert_eq!(
            index
                .document_by_duplicate_path(Path::new("archive/copy.pdf"))
                .unwrap(),
            None
        );
    }

    #[test]
    fn status_filed_survives_a_path_change() {
        let (_dir, index) = encrypted_index();
        let id = index.insert_document(&new_document(1)).unwrap();
        index
            .set_document_status(id, DocumentStatus::Filed)
            .unwrap();

        index
            .set_document_path(id, Path::new("archive/a.pdf"), stamp(1, 1))
            .unwrap();

        assert_eq!(
            index.document(id).unwrap().unwrap().status,
            DocumentStatus::Filed
        );
    }
}
