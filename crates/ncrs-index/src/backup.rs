//! The copy of the index file taken before a migration runs.

use std::path::{Path, PathBuf};

use crate::error::IndexError;

const BACKUP_INFIX: &str = ".before-v";

fn file_name_of(db_path: &Path) -> Result<String, IndexError> {
    db_path
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .ok_or_else(|| IndexError::NonUtf8Path(db_path.to_path_buf()))
}

/// The copies of `db_path` taken before migrations, beside it.
pub fn migration_backups(db_path: &Path) -> Vec<PathBuf> {
    let Some(folder) = db_path.parent() else {
        return Vec::new();
    };
    let Ok(prefix) = file_name_of(db_path).map(|name| format!("{name}{BACKUP_INFIX}")) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        .map(|entry| entry.path())
        .collect()
}

/// Copies the file as it is, so an encrypted index stays encrypted, and keeps
/// only this copy. Called when the file is at `applied` and steps are pending.
pub fn before_migration(db_path: &Path, applied: u32) -> Result<(), IndexError> {
    let older = migration_backups(db_path);
    let target =
        db_path.with_file_name(format!("{}{BACKUP_INFIX}{applied}", file_name_of(db_path)?));
    std::fs::copy(db_path, &target).map_err(IndexError::Io)?;
    for path in older.into_iter().filter(|path| *path != target) {
        std::fs::remove_file(path).map_err(IndexError::Io)?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn only_the_newest_copy_stays_and_others_beside_it_are_not_touched() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("index.db");
        std::fs::write(&db, b"content").unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"x").unwrap();
        before_migration(&db, 3).unwrap();
        before_migration(&db, 5).unwrap();
        let copies = migration_backups(&db);
        assert_eq!(copies, vec![dir.path().join("index.db.before-v5")]);
        assert_eq!(std::fs::read(&copies[0]).unwrap(), b"content");
        assert!(dir.path().join("notes.txt").exists());
    }
}
