use std::collections::{BTreeSet, HashSet};

use chrono::{NaiveDate, Utc};
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension as _};

use crate::documents::{DocumentRow, DOCUMENT_COLUMNS};
use crate::error::IndexError;
use crate::index::Index;
use crate::model::{Document, DocumentId, QuerySpec, SavedQuery, SavedQueryId, TaxContext};
use crate::names::{name_id, NameTable};

/// How many documents the full-text part of a query looks at, best match
/// first. More than any one person's archive holds, so in practice it cuts
/// nothing; it only keeps one query from reading an unbounded result.
const FULL_TEXT_HIT_LIMIT: usize = 100_000;

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

fn blank_to_none(text: &Option<String>) -> Option<&str> {
    text.as_deref()
        .map(str::trim)
        .filter(|trimmed| !trimmed.is_empty())
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
        let tx = self.conn_mut().transaction()?;
        require_free_name(&tx, name, None)?;
        tx.execute(
            "INSERT INTO saved_query (name, year, tax_context, kind, full_text, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                name,
                spec.year,
                spec.tax_context.map(TaxContext::as_str),
                blank_to_none(&spec.kind),
                full_text_words(spec),
                Utc::now().timestamp(),
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
        let tx = self.conn_mut().transaction()?;
        require_free_name(&tx, name, Some(id))?;
        let changed = tx.execute(
            "UPDATE saved_query SET name = ?2, year = ?3, tax_context = ?4, kind = ?5,
                 full_text = ?6
             WHERE id = ?1",
            params![
                id.get(),
                name,
                spec.year,
                spec.tax_context.map(TaxContext::as_str),
                blank_to_none(&spec.kind),
                full_text_words(spec),
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
            "SELECT id, name, year, tax_context, kind, full_text FROM saved_query
             ORDER BY name COLLATE NOCASE, name",
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
            .map(|(id, name, year, tax_context, kind, full_text)| {
                Ok(SavedQuery {
                    id: SavedQueryId::new(id),
                    name,
                    spec: QuerySpec {
                        year,
                        tax_context: tax_context
                            .as_deref()
                            .map(|value| TaxContext::parse("saved_query.tax_context", value))
                            .transpose()?,
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
        let mut conditions: Vec<&str> = Vec::new();
        let mut values: Vec<Value> = Vec::new();
        if let Some(year) = spec.year {
            let (Some(from), Some(until)) = (
                NaiveDate::from_ymd_opt(year, 1, 1),
                NaiveDate::from_ymd_opt(year.saturating_add(1), 1, 1),
            ) else {
                return Ok(Vec::new());
            };
            conditions.push("d.document_date >= ? AND d.document_date < ?");
            values.push(Value::Text(from.to_string()));
            values.push(Value::Text(until.to_string()));
        }
        if let Some(tax_context) = spec.tax_context {
            conditions.push("d.tax_context = ?");
            values.push(Value::Text(tax_context.as_str().to_owned()));
        }
        if let Some(kind) = blank_to_none(&spec.kind) {
            conditions.push("k.name_key = ?");
            values.push(Value::Text(kind.to_lowercase()));
        }
        for tag in spec
            .tags
            .iter()
            .map(|tag| tag.trim())
            .filter(|tag| !tag.is_empty())
        {
            conditions.push(
                "EXISTS (SELECT 1 FROM document_tag dt JOIN tag t ON t.id = dt.tag_id
                         WHERE dt.document_id = d.id AND t.name_key = ?)",
            );
            values.push(Value::Text(tag.to_lowercase()));
        }
        let text_hits: Option<HashSet<DocumentId>> = full_text_words(spec)
            .map(|words| {
                self.search_text(words, FULL_TEXT_HIT_LIMIT)
                    .map(|ids| ids.into_iter().collect())
            })
            .transpose()?;

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
            if let Some(hits) = &text_hits {
                if !hits.contains(&DocumentId::new(row.id())) {
                    continue;
                }
            }
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
    use crate::model::DocumentFields;
    use crate::test_support::{encrypted_index, new_document};

    fn tags(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    /// Six documents of different years, contexts, kinds and tags.
    fn stocked() -> (tempfile::TempDir, Index, Vec<DocumentId>) {
        let (dir, mut index) = encrypted_index();
        let stock = [
            (2024, Some(TaxContext::Company), "Invoice", &["office"][..]),
            (2024, Some(TaxContext::Private), "Invoice", &["home"][..]),
            (
                2024,
                Some(TaxContext::Company),
                "Letter",
                &["office", "tax"][..],
            ),
            (2023, Some(TaxContext::Company), "Invoice", &["office"][..]),
            (2025, Some(TaxContext::Company), "Invoice", &[][..]),
        ];
        let mut ids = Vec::new();
        for (seed, (year, context, kind, tag_names)) in (1..).zip(stock) {
            let id = index.insert_document(&new_document(seed)).unwrap();
            index
                .set_document_fields(
                    id,
                    &DocumentFields {
                        sender: Some("Company A".to_owned()),
                        kind: Some(kind.to_owned()),
                        tax_context: context,
                        date: NaiveDate::from_ymd_opt(year, 5, 1),
                        tags: tags(tag_names),
                        ..DocumentFields::default()
                    },
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
    fn a_year_and_a_tax_context_find_exactly_the_documents_of_both() {
        let (_dir, index, ids) = stocked();

        let spec = QuerySpec {
            year: Some(2024),
            tax_context: Some(TaxContext::Company),
            ..QuerySpec::default()
        };

        assert_eq!(found(&index, &spec), vec![ids[0], ids[2]]);
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
            tax_context: Some(TaxContext::Private),
            kind: Some("Invoice".to_owned()),
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
