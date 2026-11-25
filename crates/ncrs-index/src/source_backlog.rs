use std::path::{Path, PathBuf};

use rusqlite::params;

use crate::error::IndexError;
use crate::index::Index;
use crate::model::FileStamp;

fn utf8(path: &Path) -> Result<&str, IndexError> {
    path.to_str()
        .ok_or_else(|| IndexError::NonUtf8Path(path.to_path_buf()))
}

impl Index {
    /// Replaces what `source` already held when it was switched to watching.
    pub fn set_source_backlog(
        &mut self,
        source: &Path,
        files: &[(PathBuf, FileStamp)],
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
        tx.commit()?;
        Ok(())
    }

    /// Whether the file is one `source` held before watching, unchanged: the
    /// same path with another size or time is new.
    pub fn is_backlog(
        &self,
        source: &Path,
        path: &Path,
        stamp: FileStamp,
    ) -> Result<bool, IndexError> {
        let found = self.connection().query_row(
            "SELECT EXISTS (SELECT 1 FROM source_backlog
                 WHERE source = ?1 AND path = ?2 AND file_size = ?3 AND file_mtime = ?4)",
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
        self.connection()
            .execute("DELETE FROM source_backlog WHERE source = ?1", [utf8(source)?])?;
        Ok(())
    }

    pub fn forget_backlog_path(&self, source: &Path, path: &Path) -> Result<(), IndexError> {
        self.connection().execute(
            "DELETE FROM source_backlog WHERE source = ?1 AND path = ?2",
            [utf8(source)?, utf8(path)?],
        )?;
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
    fn the_same_path_with_another_size_or_time_is_not_backlog() {
        let (_dir, mut index) = encrypted_index();
        let source = Path::new("inbox");
        let file = PathBuf::from("inbox/a.pdf");
        index
            .set_source_backlog(source, &[(file.clone(), stamp(10, 100))])
            .unwrap();

        assert!(index.is_backlog(source, &file, stamp(10, 100)).unwrap());
        assert!(!index.is_backlog(source, &file, stamp(11, 100)).unwrap());
        assert!(!index.is_backlog(source, &file, stamp(10, 101)).unwrap());
        assert!(!index
            .is_backlog(Path::new("other"), &file, stamp(10, 100))
            .unwrap());
    }

    #[test]
    fn setting_replaces_and_forgetting_removes_per_source_or_path() {
        let (_dir, mut index) = encrypted_index();
        let (inbox, other) = (Path::new("inbox"), Path::new("other"));
        let (a, b) = (PathBuf::from("inbox/a.pdf"), PathBuf::from("inbox/b.pdf"));
        index
            .set_source_backlog(inbox, &[(a.clone(), stamp(1, 1)), (b.clone(), stamp(2, 2))])
            .unwrap();
        index
            .set_source_backlog(other, &[(a.clone(), stamp(1, 1))])
            .unwrap();

        index.forget_backlog_path(inbox, &a).unwrap();
        assert!(!index.is_backlog(inbox, &a, stamp(1, 1)).unwrap());
        assert!(index.is_backlog(inbox, &b, stamp(2, 2)).unwrap());

        index.set_source_backlog(inbox, &[(a.clone(), stamp(1, 1))]).unwrap();
        assert!(!index.is_backlog(inbox, &b, stamp(2, 2)).unwrap());
        assert!(index.is_backlog(inbox, &a, stamp(1, 1)).unwrap());

        index.forget_backlog(inbox).unwrap();
        assert!(!index.is_backlog(inbox, &a, stamp(1, 1)).unwrap());
        assert!(index.is_backlog(other, &a, stamp(1, 1)).unwrap());
    }
}
