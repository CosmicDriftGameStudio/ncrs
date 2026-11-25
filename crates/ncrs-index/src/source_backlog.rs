use std::path::{Path, PathBuf};

use rusqlite::{params, OptionalExtension as _};

use crate::error::IndexError;
use crate::index::Index;
use crate::model::FileStamp;

fn utf8(path: &Path) -> Result<&str, IndexError> {
    path.to_str()
        .ok_or_else(|| IndexError::NonUtf8Path(path.to_path_buf()))
}

impl Index {
    /// Replaces what `source` already held when it was switched to watching,
    /// and whether its folder was on a volume of its own then.
    pub fn set_source_backlog(
        &mut self,
        source: &Path,
        files: &[(PathBuf, FileStamp)],
        own_volume: Option<bool>,
    ) -> Result<(), IndexError> {
        let source = utf8(source)?;
        let tx = self.conn_mut().transaction()?;
        tx.execute("DELETE FROM source_backlog WHERE source = ?1", [source])?;
        let mut insert = tx.prepare(
            "INSERT OR REPLACE INTO source_backlog (source, path, file_size, file_mtime)
             VALUES (?1, ?2, ?3, ?4)",
        )?;
        for (path, stamp) in files {
            insert.execute(params![
                source,
                utf8(path)?,
                stamp.size_column(),
                stamp.mtime_nanos()
            ])?;
        }
        drop(insert);
        tx.execute(
            "INSERT OR REPLACE INTO source_watch (source, own_volume) VALUES (?1, ?2)",
            params![source, own_volume],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Whether the files of `source` were taken stock of, even if there were none.
    pub fn has_source_backlog(&self, source: &Path) -> Result<bool, IndexError> {
        let found = self.connection().query_row(
            "SELECT EXISTS (SELECT 1 FROM source_watch WHERE source = ?1)",
            [utf8(source)?],
            |row| row.get(0),
        )?;
        Ok(found)
    }

    /// Whether the folder of `source` was on a volume of its own when its
    /// files were taken stock of; `None` when that is not known.
    pub fn source_backlog_volume(&self, source: &Path) -> Result<Option<bool>, IndexError> {
        let found = self
            .connection()
            .query_row(
                "SELECT own_volume FROM source_watch WHERE source = ?1",
                [utf8(source)?],
                |row| row.get::<_, Option<bool>>(0),
            )
            .optional()?;
        Ok(found.flatten())
    }

    /// Whether the file counts as one `source` held before watching: touched
    /// at its path or renamed, with its size as it was. Another size is new.
    pub fn is_backlog(
        &self,
        source: &Path,
        path: &Path,
        stamp: FileStamp,
    ) -> Result<bool, IndexError> {
        let found = self.connection().query_row(
            "SELECT EXISTS (SELECT 1 FROM source_backlog
                 WHERE source = ?1 AND file_size = ?3 AND (path = ?2 OR file_mtime = ?4))",
            params![
                utf8(source)?,
                utf8(path)?,
                stamp.size_column(),
                stamp.mtime_nanos()
            ],
            |row| row.get(0),
        )?;
        Ok(found)
    }

    pub fn forget_backlog(&self, source: &Path) -> Result<(), IndexError> {
        let source = utf8(source)?;
        let tx = self.connection().unchecked_transaction()?;
        tx.execute("DELETE FROM source_backlog WHERE source = ?1", [source])?;
        tx.execute("DELETE FROM source_watch WHERE source = ?1", [source])?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;
    use crate::test_support::encrypted_index;

    fn stamp(size: u64, seconds: u64) -> FileStamp {
        FileStamp {
            size,
            mtime: UNIX_EPOCH + Duration::from_secs(seconds),
        }
    }

    #[test]
    fn a_backlog_file_with_another_size_or_in_another_source_is_new() {
        let (_dir, mut index) = encrypted_index();
        let source = Path::new("inbox");
        let file = PathBuf::from("inbox/a.pdf");
        index
            .set_source_backlog(source, &[(file.clone(), stamp(10, 100))], None)
            .unwrap();

        assert!(index.is_backlog(source, &file, stamp(10, 100)).unwrap());
        assert!(!index.is_backlog(source, &file, stamp(11, 100)).unwrap());
        assert!(!index
            .is_backlog(Path::new("other"), &file, stamp(10, 100))
            .unwrap());
    }

    #[test]
    fn a_backlog_file_that_was_touched_or_renamed_is_still_backlog() {
        let (_dir, mut index) = encrypted_index();
        let source = Path::new("inbox");
        let file = PathBuf::from("inbox/a.pdf");
        index
            .set_source_backlog(source, &[(file.clone(), stamp(10, 100))], None)
            .unwrap();

        assert!(index.is_backlog(source, &file, stamp(10, 101)).unwrap());
        let in_a_renamed_folder = Path::new("inbox/old/b.pdf");
        assert!(index
            .is_backlog(source, in_a_renamed_folder, stamp(10, 100))
            .unwrap());
        assert!(!index
            .is_backlog(source, in_a_renamed_folder, stamp(10, 101))
            .unwrap());
        assert!(!index
            .is_backlog(source, in_a_renamed_folder, stamp(11, 100))
            .unwrap());
    }

    #[test]
    fn setting_replaces_and_forgetting_removes_per_source() {
        let (_dir, mut index) = encrypted_index();
        let (inbox, other) = (Path::new("inbox"), Path::new("other"));
        let (a, b) = (PathBuf::from("inbox/a.pdf"), PathBuf::from("inbox/b.pdf"));
        index
            .set_source_backlog(
                inbox,
                &[(a.clone(), stamp(1, 1)), (b.clone(), stamp(2, 2))],
                Some(false),
            )
            .unwrap();
        index
            .set_source_backlog(other, &[(a.clone(), stamp(1, 1))], None)
            .unwrap();

        index
            .set_source_backlog(inbox, &[(a.clone(), stamp(1, 1))], Some(true))
            .unwrap();
        assert_eq!(index.source_backlog_volume(inbox).unwrap(), Some(true));
        assert_eq!(index.source_backlog_volume(other).unwrap(), None);
        assert!(!index.is_backlog(inbox, &b, stamp(2, 2)).unwrap());
        assert!(index.is_backlog(inbox, &a, stamp(1, 1)).unwrap());

        index.forget_backlog(inbox).unwrap();
        assert!(!index.is_backlog(inbox, &a, stamp(1, 1)).unwrap());
        assert!(index.is_backlog(other, &a, stamp(1, 1)).unwrap());
        assert!(!index.has_source_backlog(inbox).unwrap());
        assert!(index.has_source_backlog(other).unwrap());
    }

    #[test]
    fn an_empty_source_has_a_backlog_too() {
        let (_dir, mut index) = encrypted_index();
        let inbox = Path::new("inbox");
        assert!(!index.has_source_backlog(inbox).unwrap());

        index.set_source_backlog(inbox, &[], None).unwrap();

        assert!(index.has_source_backlog(inbox).unwrap());
    }
}
