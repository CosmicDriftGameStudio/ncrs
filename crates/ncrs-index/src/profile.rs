use std::path::Path;

use chrono::Utc;
use rusqlite::params;

use crate::error::IndexError;
use crate::index::Index;
use crate::model::SubtreePattern;

fn utf8(path: &Path) -> Result<&str, IndexError> {
    path.to_str()
        .ok_or_else(|| IndexError::NonUtf8Path(path.to_path_buf()))
}

impl Index {
    /// Stores what was learned about an archive location, replacing an earlier
    /// profile of the same location.
    pub fn replace_location_profile(
        &mut self,
        location: &Path,
        subtrees: &[SubtreePattern],
    ) -> Result<(), IndexError> {
        let location_text = utf8(location)?;
        let tx = self.conn_mut().transaction()?;
        tx.execute(
            "DELETE FROM location_profile WHERE location = ?1",
            [location_text],
        )?;
        tx.execute(
            "INSERT INTO location_profile (location, learned_at) VALUES (?1, ?2)",
            params![location_text, Utc::now().timestamp()],
        )?;
        for subtree in subtrees {
            tx.execute(
                "INSERT INTO location_subtree (location, subtree, pattern, documents, total)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    location_text,
                    subtree.subtree,
                    subtree.pattern,
                    subtree.documents,
                    subtree.total
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// The learned subtrees of a location, sorted by subtree; empty when the
    /// location was never learned.
    pub fn location_profile(&self, location: &Path) -> Result<Vec<SubtreePattern>, IndexError> {
        let mut statement = self.connection().prepare(
            "SELECT subtree, pattern, documents, total FROM location_subtree
             WHERE location = ?1 ORDER BY subtree",
        )?;
        let rows = statement
            .query_map([utf8(location)?], |row| {
                Ok(SubtreePattern {
                    subtree: row.get(0)?,
                    pattern: row.get(1)?,
                    documents: row.get(2)?,
                    total: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::test_support::encrypted_index;

    fn subtree(name: &str, pattern: Option<&str>, documents: u32, total: u32) -> SubtreePattern {
        SubtreePattern {
            subtree: name.to_owned(),
            pattern: pattern.map(str::to_owned),
            documents,
            total,
        }
    }

    #[test]
    fn a_stored_profile_is_read_back_sorted() {
        let (_dir, mut index) = encrypted_index();
        let location = Path::new("/archive");
        let stored = [
            subtree("Taxes", None, 0, 2),
            subtree(
                "Insurance",
                Some("{year}/Insurance/{folder}/{date}_{name}"),
                5,
                6,
            ),
        ];

        index.replace_location_profile(location, &stored).unwrap();

        assert_eq!(
            index.location_profile(location).unwrap(),
            [stored[1].clone(), stored[0].clone()]
        );
    }

    #[test]
    fn learning_again_replaces_the_earlier_profile_of_that_location_only() {
        let (_dir, mut index) = encrypted_index();
        let first = Path::new("/archive");
        let other = Path::new("/other");
        index
            .replace_location_profile(first, &[subtree("A", None, 0, 1)])
            .unwrap();
        index
            .replace_location_profile(other, &[subtree("B", None, 0, 1)])
            .unwrap();

        index
            .replace_location_profile(first, &[subtree("C", Some("{date}"), 2, 2)])
            .unwrap();

        assert_eq!(
            index.location_profile(first).unwrap(),
            [subtree("C", Some("{date}"), 2, 2)]
        );
        assert_eq!(
            index.location_profile(other).unwrap(),
            [subtree("B", None, 0, 1)]
        );
    }

    #[test]
    fn a_location_never_learned_has_an_empty_profile() {
        let (_dir, index) = encrypted_index();
        assert!(index
            .location_profile(Path::new("/archive"))
            .unwrap()
            .is_empty());
    }
}
