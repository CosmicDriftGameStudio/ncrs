//! The tags as a whole: how often each is used, which of them the file list
//! shows against its group default, and renaming or deleting a tag the user
//! made.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{params, Connection, OptionalExtension as _, Transaction};

use crate::error::IndexError;
use crate::index::Index;
use crate::model::{tag_name, ConditionKind, RuleId};

/// Per group of tags, each tag with the number of documents that have it,
/// the most used first and then by name. Grouped and spelled as
/// [`crate::FileTags`] shows them in the file list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TagTally {
    pub own: Vec<(String, u64)>,
    pub kind: Vec<(String, u64)>,
    pub sender: Vec<(String, u64)>,
    pub recipient: Vec<(String, u64)>,
    pub year: Vec<(String, u64)>,
}

#[derive(Default)]
struct Counter(BTreeMap<String, u64>);

impl Counter {
    fn add(&mut self, tag: Option<String>) {
        let spelled = tag
            .map(|text| tag_name(&text))
            .filter(|name| !name.is_empty());
        if let Some(found) = spelled {
            *self.0.entry(found).or_default() += 1;
        }
    }

    fn into_sorted(self) -> Vec<(String, u64)> {
        let mut tags: Vec<(String, u64)> = self.0.into_iter().collect();
        tags.sort_by(|(left_tag, left_count), (right_tag, right_count)| {
            right_count
                .cmp(left_count)
                .then_with(|| left_tag.cmp(right_tag))
        });
        tags
    }
}

impl Index {
    /// The tags whose visibility in the file list differs from their group's.
    pub fn file_list_tag_exceptions(&self) -> Result<BTreeMap<String, bool>, IndexError> {
        let mut statement = self
            .connection()
            .prepare("SELECT tag, shown FROM file_list_tag")?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?))
            })?
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        Ok(rows)
    }

    /// `None` removes the exception, so the tag follows its group again.
    pub fn set_file_list_tag(&self, tag: &str, shown: Option<bool>) -> Result<(), IndexError> {
        match shown {
            Some(visible) => self.connection().execute(
                "INSERT INTO file_list_tag (tag, shown) VALUES (?1, ?2)
                 ON CONFLICT (tag) DO UPDATE SET shown = excluded.shown",
                params![tag, visible],
            )?,
            None => self
                .connection()
                .execute("DELETE FROM file_list_tag WHERE tag = ?1", [tag])?,
        };
        Ok(())
    }

    pub fn clear_file_list_tags(&self, tags: &[String]) -> Result<(), IndexError> {
        let mut statement = self
            .connection()
            .prepare("DELETE FROM file_list_tag WHERE tag = ?1")?;
        for tag in tags {
            statement.execute([tag])?;
        }
        Ok(())
    }

    /// The documents, not missing, per tag and group.
    pub fn tag_tally(&self) -> Result<TagTally, IndexError> {
        let mut own = Counter::default();
        let mut kind = Counter::default();
        let mut sender = Counter::default();
        let mut recipient = Counter::default();
        let mut year = Counter::default();
        let conn = self.connection();

        let mut documents = conn.prepare(
            "SELECT k.name, d.recipient, s.name, substr(d.document_date, 1, 4)
             FROM document d
             LEFT JOIN sender s ON s.id = d.sender_id
             LEFT JOIN document_kind k ON k.id = d.kind_id
             WHERE d.missing_since IS NULL",
        )?;
        let rows = documents.query_map([], |row| {
            Ok([
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ])
        })?;
        for row in rows {
            let [kind_name, recipient_name, sender_name, year_name] = row?;
            kind.add(kind_name);
            recipient.add(recipient_name);
            sender.add(sender_name);
            year.add(year_name);
        }

        let mut own_rows = conn.prepare(
            "SELECT t.name FROM document_tag dt
             JOIN document d ON d.id = dt.document_id
             JOIN tag t ON t.id = dt.tag_id
             WHERE dt.automatic = 0 AND d.missing_since IS NULL",
        )?;
        for name in own_rows.query_map([], |row| row.get::<_, String>(0))? {
            own.add(Some(name?));
        }

        Ok(TagTally {
            own: own.into_sorted(),
            kind: kind.into_sorted(),
            sender: sender.into_sorted(),
            recipient: recipient.into_sorted(),
            year: year.into_sorted(),
        })
    }

    /// Renames a tag the user set, everywhere it stands, in one transaction.
    /// A name that already exists is merged into.
    pub fn rename_own_tag(&mut self, old_name: &str, new_name: &str) -> Result<(), IndexError> {
        let old = tag_name(old_name);
        let new = tag_name(new_name);
        if new.is_empty() {
            return Err(IndexError::EmptyName { what: "tag" });
        }
        if old == new {
            return Ok(());
        }
        let tx = self.conn_mut().transaction()?;
        if let Some(old_id) = tag_id(&tx, &old)? {
            let new_id = crate::names::name_id(&tx, crate::names::NameTable::Tag, &new)?;
            // The user's tag wins over an automatic one of the same name.
            tx.execute(
                "UPDATE document_tag SET automatic = 0 WHERE tag_id = ?2 AND document_id IN
                     (SELECT document_id FROM document_tag WHERE tag_id = ?1 AND automatic = 0)",
                [old_id, new_id],
            )?;
            tx.execute(
                "UPDATE OR IGNORE document_tag SET tag_id = ?2 WHERE tag_id = ?1 AND automatic = 0",
                [old_id, new_id],
            )?;
            tx.execute(
                "DELETE FROM document_tag WHERE tag_id = ?1 AND automatic = 0",
                [old_id],
            )?;
            for table in ["rule_tag", "saved_query_tag"] {
                tx.execute(
                    &format!("UPDATE OR IGNORE {table} SET tag_id = ?2 WHERE tag_id = ?1"),
                    [old_id, new_id],
                )?;
                tx.execute(&format!("DELETE FROM {table} WHERE tag_id = ?1"), [old_id])?;
            }
        }
        for (rule_id, position, _) in tag_conditions(&tx, &old)? {
            tx.execute(
                "UPDATE rule_condition SET value = ?3 WHERE rule_id = ?1 AND position = ?2",
                params![rule_id, position, new],
            )?;
        }
        tx.execute(
            "UPDATE OR IGNORE file_list_tag SET tag = ?2 WHERE tag = ?1",
            [&old, &new],
        )?;
        tx.execute("DELETE FROM file_list_tag WHERE tag = ?1", [&old])?;
        tx.commit()?;
        Ok(())
    }

    /// Removes a tag the user set from documents, rules that set it, saved
    /// queries and the file list exceptions. A tag a rule's condition asks for
    /// stays, and the error names those rules.
    pub fn delete_own_tag(&mut self, name: &str) -> Result<(), IndexError> {
        let tag = tag_name(name);
        let tx = self.conn_mut().transaction()?;
        let rules: BTreeSet<i64> = tag_conditions(&tx, &tag)?
            .into_iter()
            .map(|(rule_id, _, _)| rule_id)
            .collect();
        if !rules.is_empty() {
            return Err(IndexError::TagInUse {
                rules: rules.into_iter().map(RuleId::new).collect(),
            });
        }
        if let Some(id) = tag_id(&tx, &tag)? {
            tx.execute(
                "DELETE FROM document_tag WHERE tag_id = ?1 AND automatic = 0",
                [id],
            )?;
            tx.execute("DELETE FROM rule_tag WHERE tag_id = ?1", [id])?;
            tx.execute("DELETE FROM saved_query_tag WHERE tag_id = ?1", [id])?;
        }
        tx.execute("DELETE FROM file_list_tag WHERE tag = ?1", [&tag])?;
        tx.commit()?;
        Ok(())
    }
}

fn tag_id(conn: &Connection, tag: &str) -> Result<Option<i64>, IndexError> {
    Ok(conn
        .query_row(
            "SELECT id FROM tag WHERE name_key = ?1",
            [tag.to_lowercase()],
            |row| row.get(0),
        )
        .optional()?)
}

/// The conditions `(rule, position, value)` that ask for exactly this tag.
fn tag_conditions(tx: &Transaction<'_>, tag: &str) -> Result<Vec<(i64, i64, String)>, IndexError> {
    let mut statement = tx.prepare(
        "SELECT rule_id, position, value FROM rule_condition WHERE kind = ?1 ORDER BY rule_id, position",
    )?;
    let rows = statement
        .query_map([ConditionKind::Tag.as_str()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .filter(|(_, _, value)| tag_name(value) == tag)
        .collect())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use std::collections::BTreeSet;

    use chrono::NaiveDate;

    use super::*;
    use crate::test_support::{encrypted_index, new_document, rule_spec};
    use crate::{Condition, DocumentFields, FieldOrigin, QuerySpec};

    fn own(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    fn document(index: &mut Index, seed: u8, fields: DocumentFields) {
        let id = index.insert_document(&new_document(seed)).unwrap();
        index
            .set_document_fields(id, &fields, |_| FieldOrigin::Unknown)
            .unwrap();
    }

    fn tagged(names: &[&str]) -> DocumentFields {
        DocumentFields {
            tags: own(names),
            ..DocumentFields::default()
        }
    }

    fn count(index: &Index, sql: &str) -> i64 {
        index
            .connection()
            .query_row(sql, [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn exceptions_are_set_changed_and_cleared() {
        let (_dir, index) = encrypted_index();

        index.set_file_list_tag("steuer", Some(true)).unwrap();
        index.set_file_list_tag("invoice", Some(false)).unwrap();
        index.set_file_list_tag("steuer", Some(false)).unwrap();
        assert_eq!(
            index.file_list_tag_exceptions().unwrap(),
            BTreeMap::from([("invoice".to_owned(), false), ("steuer".to_owned(), false)])
        );

        index.set_file_list_tag("invoice", None).unwrap();
        index.clear_file_list_tags(&["steuer".to_owned()]).unwrap();
        assert!(index.file_list_tag_exceptions().unwrap().is_empty());
    }

    #[test]
    fn the_tally_groups_like_the_file_list_and_sorts_by_use() {
        let (_dir, mut index) = encrypted_index();
        let invoice = |sender: &str, tags: &[&str]| DocumentFields {
            sender: Some(sender.to_owned()),
            recipient: Some("Erika Beispiel".to_owned()),
            kind: Some("invoice".to_owned()),
            date: NaiveDate::from_ymd_opt(2026, 3, 4),
            tags: own(tags),
            ..DocumentFields::default()
        };
        document(&mut index, 1, invoice("Firma A GmbH", &["steuer", "miete"]));
        document(&mut index, 2, invoice("firma a gmbh", &["steuer"]));
        document(&mut index, 3, invoice("Zebra AG", &[]));

        let tally = index.tag_tally().unwrap();

        assert_eq!(
            tally.own,
            vec![("steuer".to_owned(), 2), ("miete".to_owned(), 1)]
        );
        assert_eq!(
            tally.sender,
            vec![("firma-a-gmbh".to_owned(), 2), ("zebra-ag".to_owned(), 1)]
        );
        assert_eq!(tally.recipient, vec![("erika-beispiel".to_owned(), 3)]);
        assert_eq!(tally.year, vec![("2026".to_owned(), 3)]);
        assert_eq!(tally.kind, vec![("invoice".to_owned(), 3)]);
    }

    /// A rule that sets and one that asks for the tag, and a saved query.
    fn stocked(index: &mut Index) {
        document(index, 1, tagged(&["old", "other"]));
        document(index, 2, tagged(&["old"]));
        index
            .create_rule(&crate::RuleSpec {
                tags: own(&["old"]),
                ..rule_spec()
            })
            .unwrap();
        index
            .save_query(
                "q",
                &QuerySpec {
                    tags: own(&["old"]),
                    ..QuerySpec::default()
                },
            )
            .unwrap();
        index.set_file_list_tag("old", Some(true)).unwrap();
    }

    #[test]
    fn renaming_changes_documents_rules_queries_conditions_and_exceptions() {
        let (_dir, mut index) = encrypted_index();
        stocked(&mut index);
        let mut spec = rule_spec();
        spec.conditions
            .list
            .push(Condition::new(ConditionKind::Tag, "old"));
        let asking = index.create_rule(&spec).unwrap();

        index.rename_own_tag("old", "New Name").unwrap();

        assert_eq!(count(&index, "SELECT count(*) FROM tag t JOIN document_tag dt ON dt.tag_id = t.id WHERE t.name = 'new-name' AND dt.automatic = 0"), 2);
        assert_eq!(
            index.tag_tally().unwrap().own.first(),
            Some(&("new-name".to_owned(), 2))
        );
        let rules = index.rules().unwrap();
        assert!(rules.iter().all(|rule| !rule.spec.tags.contains("old")));
        assert!(rules.iter().any(|rule| rule.spec.tags.contains("new-name")));
        let condition = &index.rule(asking).unwrap().unwrap().spec.conditions.list;
        assert!(condition.iter().any(|found| found.value == "new-name"));
        assert_eq!(
            index.saved_queries().unwrap()[0].spec.tags,
            own(&["new-name"])
        );
        assert_eq!(
            index.file_list_tag_exceptions().unwrap(),
            BTreeMap::from([("new-name".to_owned(), true)])
        );
    }

    #[test]
    fn renaming_into_an_existing_name_merges_and_an_empty_name_is_refused() {
        let (_dir, mut index) = encrypted_index();
        document(&mut index, 1, tagged(&["a", "b"]));
        document(&mut index, 2, tagged(&["a"]));
        document(&mut index, 3, tagged(&["b"]));

        index.rename_own_tag("a", "b").unwrap();

        assert_eq!(index.tag_tally().unwrap().own, vec![("b".to_owned(), 3)]);
        let err = index.rename_own_tag("b", " - ").unwrap_err();
        assert!(
            matches!(err, IndexError::EmptyName { what: "tag" }),
            "{err}"
        );
        assert_eq!(index.tag_tally().unwrap().own, vec![("b".to_owned(), 3)]);
    }

    #[test]
    fn deleting_removes_the_tag_from_documents_rules_queries_and_exceptions() {
        let (_dir, mut index) = encrypted_index();
        stocked(&mut index);

        index.delete_own_tag("old").unwrap();

        assert_eq!(
            index.tag_tally().unwrap().own,
            vec![("other".to_owned(), 1)]
        );
        assert!(index.rules().unwrap()[0].spec.tags.is_empty());
        assert!(index.saved_queries().unwrap()[0].spec.tags.is_empty());
        assert!(index.file_list_tag_exceptions().unwrap().is_empty());
    }

    #[test]
    fn a_tag_a_rule_condition_asks_for_is_neither_deleted_nor_renamed_away_half() {
        let (_dir, mut index) = encrypted_index();
        stocked(&mut index);
        let mut spec = rule_spec();
        spec.conditions
            .list
            .push(Condition::new(ConditionKind::Tag, "Old"));
        let asking = index.create_rule(&spec).unwrap();

        let err = index.delete_own_tag("old").unwrap_err();

        assert!(
            matches!(&err, IndexError::TagInUse { rules } if *rules == vec![asking]),
            "{err}"
        );
        assert_eq!(
            index.tag_tally().unwrap().own.first(),
            Some(&("old".to_owned(), 2))
        );
        assert_eq!(
            index.file_list_tag_exceptions().unwrap(),
            BTreeMap::from([("old".to_owned(), true)])
        );
    }
}
