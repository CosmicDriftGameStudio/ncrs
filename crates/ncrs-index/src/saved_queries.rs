use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use chrono::{NaiveDate, Utc};
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension as _};

use crate::documents::{match_query, DocumentRow, DOCUMENT_COLUMNS};
use crate::error::IndexError;
use crate::hermes_column::SubtreeRange;
use crate::index::Index;
use crate::model::{tag_name, Document, QuerySpec, SavedQuery, SavedQueryId};
use crate::names::{name_id, NameTable};

/// The words of a full-text criterion, or `None` when it is blank: a blank
/// criterion asks for nothing, it does not match nothing.
fn full_text_words(spec: &QuerySpec) -> Option<&str> {
    spec.full_text
        .as_deref()
        .map(str::trim)
        .filter(|words| !words.is_empty())
}

fn clean_name(raw: &str) -> Result<&str, IndexError> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(IndexError::EmptyName {
            what: "saved query",
        });
    }
    Ok(name)
}

fn replace_query_tags(
    conn: &Connection,
    id: SavedQueryId,
    tags: &BTreeSet<String>,
) -> Result<(), IndexError> {
    conn.execute(
        "DELETE FROM saved_query_tag WHERE saved_query_id = ?1",
        [id.get()],
    )?;
    for tag in tags {
        let tag_id = name_id(conn, NameTable::Tag, tag)?;
        conn.execute(
            "INSERT OR IGNORE INTO saved_query_tag (saved_query_id, tag_id) VALUES (?1, ?2)",
            [id.get(), tag_id],
        )?;
    }
    Ok(())
}

fn require_free_name(
    conn: &Connection,
    name: &str,
    own: Option<SavedQueryId>,
) -> Result<(), IndexError> {
    let holder: Option<i64> = conn
        .query_row(
            "SELECT id FROM saved_query WHERE name = ?1",
            [name],
            |row| row.get(0),
        )
        .optional()?;
    match holder {
        Some(taken_by) if Some(taken_by) != own.map(SavedQueryId::get) => {
            Err(IndexError::SavedQueryNameTaken(name.to_owned()))
        }
        _ => Ok(()),
    }
}

/// The text as a LIKE pattern that takes `\`, `%` and `_` literally.
fn escape_like(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(character, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn blank_to_none(text: &Option<String>) -> Option<&str> {
    text.as_deref()
        .map(str::trim)
        .filter(|trimmed| !trimmed.is_empty())
}

/// `?, ?, ?` for `count` bound values; `NULL` for none, so `IN (NULL)`
/// is valid and matches nothing.
fn placeholders(count: usize) -> String {
    if count == 0 {
        return "NULL".to_owned();
    }
    vec!["?"; count].join(", ")
}

impl Index {
    /// A name that is already taken is an error, not an update: two queries
    /// of one name could not be told apart in the list.
    pub fn save_query(
        &mut self,
        raw_name: &str,
        spec: &QuerySpec,
    ) -> Result<SavedQueryId, IndexError> {
        let name = clean_name(raw_name)?;
        let kinds = Arc::clone(self.kinds());
        let tx = self.conn_mut().transaction()?;
        require_free_name(&tx, name, None)?;
        tx.execute(
            "INSERT INTO saved_query (name, year, kind, full_text, created_at, sender)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                name,
                spec.year,
                blank_to_none(&spec.kind).map(|kind| kinds.stored_kind(kind)),
                full_text_words(spec),
                Utc::now().timestamp(),
                blank_to_none(&spec.sender),
            ],
        )?;
        let id = SavedQueryId::new(tx.last_insert_rowid());
        replace_query_tags(&tx, id, &spec.tags)?;
        tx.commit()?;
        Ok(id)
    }

    pub fn update_saved_query(
        &mut self,
        id: SavedQueryId,
        raw_name: &str,
        spec: &QuerySpec,
    ) -> Result<(), IndexError> {
        let name = clean_name(raw_name)?;
        let kinds = Arc::clone(self.kinds());
        let tx = self.conn_mut().transaction()?;
        require_free_name(&tx, name, Some(id))?;
        let changed = tx.execute(
            "UPDATE saved_query SET name = ?2, year = ?3, kind = ?4,
                 full_text = ?5, sender = ?6
             WHERE id = ?1",
            params![
                id.get(),
                name,
                spec.year,
                blank_to_none(&spec.kind).map(|kind| kinds.stored_kind(kind)),
                full_text_words(spec),
                blank_to_none(&spec.sender),
            ],
        )?;
        if changed == 0 {
            return Err(IndexError::SavedQueryNotFound(id));
        }
        replace_query_tags(&tx, id, &spec.tags)?;
        tx.commit()?;
        Ok(())
    }

    /// Whether a saved query was there to delete.
    pub fn delete_saved_query(&self, id: SavedQueryId) -> Result<bool, IndexError> {
        let changed = self
            .connection()
            .execute("DELETE FROM saved_query WHERE id = ?1", [id.get()])?;
        Ok(changed > 0)
    }

    /// Ordered by name.
    pub fn saved_queries(&self) -> Result<Vec<SavedQuery>, IndexError> {
        let mut statement = self.connection().prepare(
            "SELECT q.id, q.name, q.year, q.kind, q.full_text, q.sender FROM saved_query q
             ORDER BY q.name COLLATE NOCASE, q.name",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i32>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(id, name, year, kind, full_text, sender)| {
                Ok(SavedQuery {
                    id: SavedQueryId::new(id),
                    name,
                    spec: QuerySpec {
                        year,
                        sender,
                        kind,
                        tags: self.saved_query_tags(id)?,
                        full_text,
                    },
                })
            })
            .collect()
    }

    /// The documents that fit every criterion, by document date (those
    /// without one last) and path. Missing documents are included; the caller
    /// sees them by `missing_since`.
    pub fn run_query(&self, spec: &QuerySpec) -> Result<Vec<Document>, IndexError> {
        self.run_query_under(spec, None)
    }

    /// Like [`Self::run_query`], and only the documents below the folder
    /// `under`. The folder is a condition of the query like the others, so
    /// nothing is cut before it applies.
    pub fn run_query_under(
        &self,
        spec: &QuerySpec,
        under: Option<&Path>,
    ) -> Result<Vec<Document>, IndexError> {
        let mut conditions: Vec<String> = Vec::new();
        let mut values: Vec<Value> = Vec::new();
        if let Some(year) = spec.year {
            let (Some(from), Some(until)) = (
                NaiveDate::from_ymd_opt(year, 1, 1),
                NaiveDate::from_ymd_opt(year.saturating_add(1), 1, 1),
            ) else {
                return Ok(Vec::new());
            };
            conditions.push("d.document_date >= ? AND d.document_date < ?".to_owned());
            values.push(Value::Text(from.to_string()));
            values.push(Value::Text(until.to_string()));
        }
        if let Some(sender) = blank_to_none(&spec.sender) {
            conditions.push("s.name_key = ?".to_owned());
            values.push(Value::Text(sender.to_lowercase()));
        }
        if let Some(kind) = blank_to_none(&spec.kind) {
            let keys = self.kinds().kind_name_keys(kind);
            conditions.push(format!("k.name_key IN ({})", placeholders(keys.len())));
            values.extend(keys.into_iter().map(Value::Text));
        }
        for tag in spec
            .tags
            .iter()
            .map(|tag| tag_name(tag))
            .filter(|tag| !tag.is_empty())
        {
            let other_spellings = self.kinds().tag_spellings(&tag);
            conditions.push(format!(
                "EXISTS (SELECT 1 FROM document_tag dt JOIN tag t ON t.id = dt.tag_id
                         WHERE dt.document_id = d.id
                           AND (t.name_key = ? OR t.name_key LIKE ? ESCAPE '\\'
                                OR t.name_key IN ({})))",
                placeholders(other_spellings.len())
            ));
            values.push(Value::Text(tag.clone()));
            values.push(Value::Text(format!("{}-%", escape_like(&tag))));
            values.extend(other_spellings.into_iter().map(Value::Text));
        }
        if let Some(words) = full_text_words(spec) {
            let query = match_query(words, true);
            if query.is_empty() {
                return Ok(Vec::new());
            }
            conditions.push(
                "d.id IN (SELECT rowid FROM document_text WHERE document_text MATCH ?)".to_owned(),
            );
            values.push(Value::Text(query));
        }
        if let Some(folder) = under {
            let range = SubtreeRange::of(folder)?;
            conditions.push("d.path >= ? AND d.path < ?".to_owned());
            let (from, until) = range.into_bounds();
            values.push(Value::Text(from));
            values.push(Value::Text(until));
        }

        let mut sql = DOCUMENT_COLUMNS.to_owned();
        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }
        sql.push_str(" ORDER BY d.document_date IS NULL, d.document_date, d.path, d.id");

        let mut statement = self.connection().prepare(&sql)?;
        let rows = statement
            .query_map(params_from_iter(values), DocumentRow::read)?
            .collect::<Result<Vec<_>, _>>()?;
        let mut documents = Vec::new();
        for row in rows {
            let tags = self.document_tags(row.id())?;
            documents.push(row.into_document(tags)?);
        }
        Ok(documents)
    }

    fn saved_query_tags(&self, id: i64) -> Result<BTreeSet<String>, IndexError> {
        let mut statement = self.connection().prepare(
            "SELECT t.name FROM saved_query_tag qt JOIN tag t ON t.id = qt.tag_id
             WHERE qt.saved_query_id = ?1",
        )?;
        let tags = statement
            .query_map([id], |row| row.get(0))?
            .collect::<Result<BTreeSet<String>, _>>()?;
        Ok(tags)
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
    use chrono::NaiveDate;

    use super::*;
    use crate::model::{DocumentFields, DocumentId, FieldOrigin};
    use crate::test_support::{encrypted_index, new_document};

    fn tags(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    /// Six documents of different years, kinds and tags.
    fn stocked() -> (tempfile::TempDir, Index, Vec<DocumentId>) {
        let (dir, mut index) = encrypted_index();
        let stock = [
            (2024, "Invoice", &["office"][..]),
            (2024, "Invoice", &["home"][..]),
            (2024, "Letter", &["office", "tax"][..]),
            (2023, "Invoice", &["office"][..]),
            (2025, "Invoice", &[][..]),
        ];
        let mut ids = Vec::new();
        let senders = [
            "Company A",
            "Company A",
            "Company A",
            "Company A",
            "Other Ltd",
        ];
        for (seed, ((year, kind, tag_names), sender)) in (1..).zip(stock.into_iter().zip(senders)) {
            let id = index.insert_document(&new_document(seed)).unwrap();
            index
                .set_document_fields(
                    id,
                    &DocumentFields {
                        sender: Some(sender.to_owned()),
                        kind: Some(kind.to_owned()),
                        date: NaiveDate::from_ymd_opt(year, 5, 1),
                        tags: tags(tag_names),
                        ..DocumentFields::default()
                    },
                    |_| FieldOrigin::Unknown,
                )
                .unwrap();
            ids.push(id);
        }
        let undated = index.insert_document(&new_document(9)).unwrap();
        ids.push(undated);
        (dir, index, ids)
    }

    fn found(index: &Index, spec: &QuerySpec) -> Vec<DocumentId> {
        index
            .run_query(spec)
            .unwrap()
            .into_iter()
            .map(|document| document.id)
            .collect()
    }

    #[test]
    fn no_criterion_returns_everything_dated_first_then_the_undated() {
        let (_dir, index, ids) = stocked();

        assert_eq!(
            found(&index, &QuerySpec::default()),
            vec![ids[3], ids[0], ids[1], ids[2], ids[4], ids[5]]
        );
    }

    #[test]
    fn the_kind_ignores_case_and_every_tag_is_needed() {
        let (_dir, index, ids) = stocked();

        let by_kind = QuerySpec {
            kind: Some("invoice".to_owned()),
            year: Some(2024),
            ..QuerySpec::default()
        };
        let by_tags = QuerySpec {
            tags: tags(&["OFFICE", "tax"]),
            ..QuerySpec::default()
        };

        assert_eq!(found(&index, &by_kind), vec![ids[0], ids[1]]);
        assert_eq!(found(&index, &by_tags), vec![ids[2]]);
    }

    fn stocked_with_tags(own: &[&[&str]]) -> (tempfile::TempDir, Index, Vec<DocumentId>) {
        let (dir, mut index) = encrypted_index();
        let mut ids = Vec::new();
        for (seed, names) in (1..).zip(own) {
            let id = index.insert_document(&new_document(seed)).unwrap();
            index
                .set_document_fields(
                    id,
                    &DocumentFields {
                        kind: Some("Invoice".to_owned()),
                        date: NaiveDate::from_ymd_opt(2026, 2, 3),
                        tags: tags(names),
                        ..DocumentFields::default()
                    },
                    |_| FieldOrigin::Unknown,
                )
                .unwrap();
            ids.push(id);
        }
        (dir, index, ids)
    }

    #[test]
    fn a_tag_finds_the_more_specific_ones_below_it_but_not_a_longer_name() {
        let (_dir, index, ids) =
            stocked_with_tags(&[&["firma-a-gmbh"], &["firma-a"], &["firma-ab"], &[]]);

        let spec = QuerySpec {
            tags: tags(&["Firma A"]),
            ..QuerySpec::default()
        };

        assert_eq!(found(&index, &spec), vec![ids[0], ids[1]]);
    }

    #[test]
    fn like_characters_are_escaped_for_the_prefix_match() {
        assert_eq!(escape_like(r"a_b%c\d"), r"a\_b\%c\\d");
    }

    #[test]
    fn the_automatic_tags_of_kind_and_year_are_found() {
        let (_dir, index, ids) = stocked_with_tags(&[&[], &["steuer"]]);

        let spec = QuerySpec {
            tags: tags(&["invoice", "2026"]),
            ..QuerySpec::default()
        };

        assert_eq!(found(&index, &spec), vec![ids[0], ids[1]]);
    }

    #[test]
    fn a_custom_kind_is_found_by_its_tag_and_its_words() {
        let (_dir, mut index) = encrypted_index();
        let (catalog, skips) = crate::KindCatalog::built_in().extended(&[crate::CustomKind {
            id: "payslip".to_owned(),
            german: Some("Lohnabrechnung".to_owned()),
            english: Some("Payslip".to_owned()),
            words: vec!["Gehaltsabrechnung".to_owned()],
        }]);
        assert_eq!(skips, Vec::new());
        index.set_kinds(Arc::new(catalog));
        let mut ids = Vec::new();
        for (seed, kind) in [(1, "Gehaltsabrechnung"), (2, "Invoice")] {
            let id = index.insert_document(&new_document(seed)).unwrap();
            let fields = DocumentFields {
                kind: Some(kind.to_owned()),
                ..DocumentFields::default()
            };
            index
                .set_document_fields(id, &fields, |_| FieldOrigin::Unknown)
                .unwrap();
            ids.push(id);
        }

        let by_tag = QuerySpec {
            tags: tags(&["lohnabrechnung"]),
            ..QuerySpec::default()
        };
        let by_kind = QuerySpec {
            kind: Some("Lohnabrechnung".to_owned()),
            ..QuerySpec::default()
        };

        assert_eq!(found(&index, &by_tag), vec![ids[0]]);
        assert_eq!(found(&index, &by_kind), vec![ids[0]]);
    }

    #[test]
    fn a_known_kind_is_found_by_every_spelling_it_is_stored_under() {
        let (_dir, mut index) = encrypted_index();
        let mut ids = Vec::new();
        for (seed, kind) in [(1, "invoice"), (2, "Placeholder")] {
            let id = index.insert_document(&new_document(seed)).unwrap();
            let fields = DocumentFields {
                kind: Some(kind.to_owned()),
                ..DocumentFields::default()
            };
            index
                .set_document_fields(id, &fields, |_| FieldOrigin::Unknown)
                .unwrap();
            ids.push(id);
        }
        index
            .connection()
            .execute_batch(
                "UPDATE document_kind SET name = 'Rechnung', name_key = 'rechnung'
                     WHERE name_key = 'placeholder';
                 UPDATE tag SET name = 'rechnung', name_key = 'rechnung'
                     WHERE name_key = 'placeholder';",
            )
            .unwrap();

        for spec in [
            QuerySpec {
                kind: Some("invoice".to_owned()),
                ..QuerySpec::default()
            },
            QuerySpec {
                kind: Some("Rechnung".to_owned()),
                ..QuerySpec::default()
            },
            QuerySpec {
                tags: tags(&["rechnung"]),
                ..QuerySpec::default()
            },
            QuerySpec {
                tags: tags(&["Invoice"]),
                ..QuerySpec::default()
            },
        ] {
            assert_eq!(found(&index, &spec), ids, "{spec:?}");
        }
    }

    #[test]
    fn the_full_text_narrows_the_other_criteria() {
        let (_dir, mut index, ids) = stocked();
        index
            .set_document_text(ids[0], "Rent for the office in May")
            .unwrap();
        index
            .set_document_text(ids[3], "Rent for the office in May")
            .unwrap();
        index.set_document_text(ids[1], "Groceries").unwrap();

        let spec = QuerySpec {
            full_text: Some("rent office".to_owned()),
            year: Some(2024),
            ..QuerySpec::default()
        };

        assert_eq!(found(&index, &spec), vec![ids[0]]);
        let blank = QuerySpec {
            full_text: Some("  ".to_owned()),
            year: Some(2025),
            ..QuerySpec::default()
        };
        assert_eq!(found(&index, &blank), vec![ids[4]]);
    }

    #[test]
    fn a_folder_keeps_the_documents_below_it_and_the_full_text_works_inside_it() {
        let (_dir, mut index, ids) = stocked();
        index
            .set_document_text(ids[0], "Rent for the office")
            .unwrap();
        index
            .set_document_text(ids[1], "Rent for the office")
            .unwrap();
        let spec = QuerySpec {
            full_text: Some("rent".to_owned()),
            ..QuerySpec::default()
        };
        let under = |folder: &str| -> Vec<DocumentId> {
            index
                .run_query_under(&spec, Some(Path::new(folder)))
                .unwrap()
                .into_iter()
                .map(|document| document.id)
                .collect()
        };

        assert_eq!(under("inbox"), vec![ids[0], ids[1]]);
        assert_eq!(under("inbox/"), vec![ids[0], ids[1]]);
        assert!(under("inbo").is_empty());
        assert!(under("inbox/scan-1.pdf/deeper").is_empty());
    }

    #[test]
    fn the_sender_ignores_case_and_finds_exactly_its_documents() {
        let (_dir, index, ids) = stocked();

        let spec = QuerySpec {
            sender: Some("  company a ".to_owned()),
            ..QuerySpec::default()
        };
        let other = QuerySpec {
            sender: Some("OTHER LTD".to_owned()),
            ..QuerySpec::default()
        };

        assert_eq!(found(&index, &spec), vec![ids[3], ids[0], ids[1], ids[2]]);
        assert_eq!(found(&index, &other), vec![ids[4]]);
    }

    #[test]
    fn the_last_word_of_the_full_text_may_begin_a_longer_word() {
        let (_dir, mut index, ids) = stocked();
        index
            .set_document_text(ids[0], "Rent for the office in May")
            .unwrap();

        let by_start = QuerySpec {
            full_text: Some("office ren".to_owned()),
            ..QuerySpec::default()
        };
        let by_start_of_first = QuerySpec {
            full_text: Some("off rent".to_owned()),
            ..QuerySpec::default()
        };

        assert_eq!(found(&index, &by_start), vec![ids[0]]);
        assert!(found(&index, &by_start_of_first).is_empty());
    }

    #[test]
    fn user_input_is_a_value_never_sql() {
        let (_dir, index, _ids) = stocked();

        let spec = QuerySpec {
            kind: Some("x' OR '1'='1".to_owned()),
            tags: tags(&["a') OR 1=1 --"]),
            ..QuerySpec::default()
        };

        assert!(found(&index, &spec).is_empty());
    }

    #[test]
    fn a_missing_document_is_part_of_the_answer() {
        let (_dir, index, ids) = stocked();
        index
            .connection()
            .execute(
                "UPDATE document SET missing_since = 1 WHERE id = ?1",
                [ids[4].get()],
            )
            .unwrap();

        let spec = QuerySpec {
            year: Some(2025),
            ..QuerySpec::default()
        };
        let documents = index.run_query(&spec).unwrap();

        assert_eq!(documents.len(), 1);
        assert!(documents[0].missing_since.is_some());
    }

    #[test]
    fn queries_are_saved_changed_deleted_and_listed_by_name() {
        let (_dir, mut index) = encrypted_index();
        let spec = QuerySpec {
            year: Some(2024),
            sender: Some("Company A".to_owned()),
            kind: Some("invoice".to_owned()),
            tags: tags(&["home"]),
            full_text: Some("rent".to_owned()),
        };

        let second = index.save_query("Year B", &spec).unwrap();
        let first = index.save_query("year a", &QuerySpec::default()).unwrap();

        let listed = index.saved_queries().unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|query| query.name.as_str())
                .collect::<Vec<_>>(),
            ["year a", "Year B"]
        );
        assert_eq!(listed[1].spec, spec);

        let changed = QuerySpec {
            year: Some(2023),
            ..QuerySpec::default()
        };
        index
            .update_saved_query(second, "Year C", &changed)
            .unwrap();
        assert_eq!(index.saved_queries().unwrap()[1].spec, changed);
        assert_eq!(index.saved_queries().unwrap()[1].name, "Year C");

        assert!(index.delete_saved_query(first).unwrap());
        assert!(!index.delete_saved_query(first).unwrap());
        assert_eq!(index.saved_queries().unwrap().len(), 1);
    }

    #[test]
    fn a_taken_or_empty_name_is_an_error_and_changes_nothing() {
        let (_dir, mut index) = encrypted_index();
        let first = index.save_query("Year A", &QuerySpec::default()).unwrap();
        let second = index.save_query("Year B", &QuerySpec::default()).unwrap();

        assert!(matches!(
            index.save_query(" Year A ", &QuerySpec::default()),
            Err(IndexError::SavedQueryNameTaken(_))
        ));
        assert!(matches!(
            index.update_saved_query(second, "Year A", &QuerySpec::default()),
            Err(IndexError::SavedQueryNameTaken(_))
        ));
        assert!(matches!(
            index.save_query("  ", &QuerySpec::default()),
            Err(IndexError::EmptyName { .. })
        ));
        // Keeping its own name is no clash.
        index
            .update_saved_query(
                first,
                "Year A",
                &QuerySpec {
                    year: Some(1),
                    ..QuerySpec::default()
                },
            )
            .unwrap();
        assert!(matches!(
            index.update_saved_query(SavedQueryId::new(999), "Zed", &QuerySpec::default()),
            Err(IndexError::SavedQueryNotFound(_))
        ));
        assert_eq!(index.saved_queries().unwrap().len(), 2);
    }
}
