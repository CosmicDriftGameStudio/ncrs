use std::collections::HashMap;
use std::path::Path;

use rusqlite::{params, Connection};

use crate::error::IndexError;
use crate::index::Index;
use crate::model::ContentHash;

/// A cache of what the model answered, so it is not asked again. It is no
/// numbered migration: that would raise `user_version`, and the installed
/// older app would then refuse the index (`IndexError::SchemaTooNew`).
pub(crate) fn create_folder_pick_table(conn: &Connection) -> Result<(), IndexError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS folder_pick (
             hash BLOB NOT NULL,
             target_root TEXT NOT NULL,
             folder TEXT,
             version INTEGER NOT NULL,
             PRIMARY KEY (hash, target_root)
         )",
    )?;
    Ok(())
}

fn root_text(target_root: &Path) -> String {
    target_root.to_string_lossy().into_owned()
}

impl Index {
    /// The folders the model picked for these files below `target_root`, as
    /// asked with `version` of the prompt. `None` as the folder: the model
    /// found none that fits. Files without a stored pick are left out.
    pub fn folder_picks(
        &self,
        target_root: &Path,
        hashes: &[ContentHash],
        version: i64,
    ) -> Result<HashMap<ContentHash, Option<String>>, IndexError> {
        let mut statement = self.connection().prepare(
            "SELECT folder FROM folder_pick
             WHERE hash = ?1 AND target_root = ?2 AND version = ?3",
        )?;
        let root = root_text(target_root);
        let mut found = HashMap::new();
        for hash in hashes {
            let mut rows = statement.query(params![hash.0.as_slice(), root, version])?;
            if let Some(row) = rows.next()? {
                found.insert(*hash, row.get::<_, Option<String>>(0)?);
            }
        }
        Ok(found)
    }

    /// Stores the picks, replacing what was stored for the same file and
    /// target.
    pub fn store_folder_picks(
        &self,
        target_root: &Path,
        picks: &[(ContentHash, Option<String>)],
        version: i64,
    ) -> Result<(), IndexError> {
        let transaction = self.connection().unchecked_transaction()?;
        {
            let mut statement = transaction.prepare(
                "INSERT INTO folder_pick (hash, target_root, folder, version)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (hash, target_root)
                 DO UPDATE SET folder = excluded.folder, version = excluded.version",
            )?;
            let root = root_text(target_root);
            for (hash, folder) in picks {
                statement.execute(params![hash.0.as_slice(), root, folder, version])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::test_support::plain_index;

    fn hash(seed: u8) -> ContentHash {
        ContentHash([seed; 32])
    }

    #[test]
    fn stored_picks_are_read_back() {
        let (_dir, index) = plain_index();
        let root = PathBuf::from("/archive");
        index
            .store_folder_picks(&root, &[(hash(1), Some("Bank/2022".to_owned()))], 1)
            .unwrap();

        let found = index.folder_picks(&root, &[hash(1), hash(2)], 1).unwrap();

        assert_eq!(found.len(), 1);
        assert_eq!(found.get(&hash(1)), Some(&Some("Bank/2022".to_owned())));
    }

    #[test]
    fn another_version_or_target_is_not_returned() {
        let (_dir, index) = plain_index();
        let root = PathBuf::from("/archive");
        index
            .store_folder_picks(&root, &[(hash(1), Some("Bank".to_owned()))], 1)
            .unwrap();

        assert!(index.folder_picks(&root, &[hash(1)], 2).unwrap().is_empty());
        assert!(index
            .folder_picks(Path::new("/other"), &[hash(1)], 1)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn no_folder_round_trips() {
        let (_dir, index) = plain_index();
        let root = PathBuf::from("/archive");
        index
            .store_folder_picks(&root, &[(hash(1), None)], 1)
            .unwrap();

        let found = index.folder_picks(&root, &[hash(1)], 1).unwrap();

        assert_eq!(found.get(&hash(1)), Some(&None));
    }

    #[test]
    fn storing_again_replaces_the_pick() {
        let (_dir, index) = plain_index();
        let root = PathBuf::from("/archive");
        index
            .store_folder_picks(&root, &[(hash(1), Some("Bank".to_owned()))], 1)
            .unwrap();
        index
            .store_folder_picks(&root, &[(hash(1), None)], 1)
            .unwrap();

        let found = index.folder_picks(&root, &[hash(1)], 1).unwrap();

        assert_eq!(found.get(&hash(1)), Some(&None));
    }
}
