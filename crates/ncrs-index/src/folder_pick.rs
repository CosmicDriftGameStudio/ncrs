use std::collections::HashMap;
use std::path::Path;

use rusqlite::{params, Connection};

use crate::error::IndexError;
use crate::index::Index;
use crate::model::ContentHash;

/// A cache of what the model answered, so it is not asked again. It is no
/// numbered migration: that would raise `user_version`, and the installed
/// older app would then refuse the index (`IndexError::SchemaTooNew`).
pub fn create_folder_pick_table(conn: &Connection) -> Result<(), IndexError> {
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
    /// Drops the "no folder fits" picks stored before `version` and takes
    /// `tags` off the files they were stored for: an answer of a model that
    /// could not choose folders is no answer. Returns how many picks went.
    pub fn forget_empty_picks_before(
        &self,
        version: i64,
        tags: &[String],
    ) -> Result<usize, IndexError> {
        let mut statement = self.connection().prepare(
            "SELECT DISTINCT hash FROM folder_pick
             WHERE folder IS NULL AND version < ?1
               AND hash NOT IN (
                   SELECT hash FROM folder_pick WHERE folder IS NULL AND version >= ?1
               )",
        )?;
        let hashes: Vec<[u8; 32]> = statement
            .query_map([version], |row| row.get::<_, [u8; 32]>(0))?
            .collect::<Result<_, _>>()?;
        for hash in &hashes {
            for tag in tags {
                self.remove_document_tag(&ContentHash(*hash), tag)?;
            }
        }
        let dropped = self.connection().execute(
            "DELETE FROM folder_pick WHERE folder IS NULL AND version < ?1",
            [version],
        )?;
        Ok(dropped)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
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
    #[test]
    fn empty_picks_of_an_older_version_go_with_their_tags_and_the_rest_stays() {
        let (_dir, index) = plain_index();
        let root = PathBuf::from("/archive");
        let stale = crate::test_support::new_document(1).hash;
        let kept = crate::test_support::new_document(2).hash;
        let stale_id = index
            .insert_document(&crate::test_support::new_document(1))
            .unwrap();
        let kept_id = index
            .insert_document(&crate::test_support::new_document(2))
            .unwrap();
        index.add_document_tag(&stale, "Hermes: no folder").unwrap();
        index.add_document_tag(&kept, "Hermes: no folder").unwrap();
        index
            .store_folder_picks(&root, &[(stale, None), (kept, Some("Bank".to_owned()))], 1)
            .unwrap();
        index.store_folder_picks(&root, &[(kept, None)], 2).unwrap();

        let dropped = index
            .forget_empty_picks_before(2, &["Hermes: no folder".to_owned()])
            .unwrap();

        assert_eq!(dropped, 1);
        assert!(index.document_tags(stale_id.get()).unwrap().is_empty());
        assert!(!index.document_tags(kept_id.get()).unwrap().is_empty());
        assert!(index.folder_picks(&root, &[stale], 1).unwrap().is_empty());
        assert_eq!(index.folder_picks(&root, &[kept], 2).unwrap().len(), 1);
    }

    #[test]
    fn a_tag_stays_while_another_target_has_a_valid_empty_pick_for_the_file() {
        let (_dir, index) = plain_index();
        let old_root = PathBuf::from("/old");
        let new_root = PathBuf::from("/new");
        let hash = crate::test_support::new_document(1).hash;
        let id = index
            .insert_document(&crate::test_support::new_document(1))
            .unwrap();
        index.add_document_tag(&hash, "Hermes: no folder").unwrap();
        index
            .store_folder_picks(&old_root, &[(hash, None)], 1)
            .unwrap();
        index
            .store_folder_picks(&new_root, &[(hash, None)], 2)
            .unwrap();

        let dropped = index
            .forget_empty_picks_before(2, &["Hermes: no folder".to_owned()])
            .unwrap();

        assert_eq!(dropped, 1);
        assert!(!index.document_tags(id.get()).unwrap().is_empty());
        assert!(index
            .folder_picks(&new_root, &[hash], 2)
            .unwrap()
            .contains_key(&hash));
    }
}
