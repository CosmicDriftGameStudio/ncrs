use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension as _};

use crate::error::IndexError;
use crate::index::Index;
use crate::model::{
    corrupt, currency_from_column, date_from_column, hash_from_column, timestamp_from_column,
    Amount, ContentHash, Document, DocumentFields, DocumentId, DocumentSource, DocumentStatus,
    Field, FieldOrigin, FieldOrigins, FileStamp, NewDocument, TaxContext,
};
use crate::names::{name_id, optional_name_id, NameTable};
use crate::tax_contexts::optional_tax_context_id;

pub const DOCUMENT_COLUMNS: &str = "SELECT d.id, d.hash, d.path, d.source, d.received_at, d.status,
        s.name, k.name, tc.short_name, d.document_date, d.document_number,
        d.amount_minor, d.currency, d.file_size, d.file_mtime, d.missing_since
    FROM document d
    LEFT JOIN sender s ON s.id = d.sender_id
    LEFT JOIN document_kind k ON k.id = d.kind_id
    LEFT JOIN tax_context tc ON tc.id = d.tax_context_id";

/// A row as stored, before its text columns are parsed into the model types.
pub struct DocumentRow {
    id: i64,
    hash: Vec<u8>,
    path: String,
    source: String,
    received_at: i64,
    status: String,
    sender: Option<String>,
    kind: Option<String>,
    tax_context: Option<String>,
    date: Option<String>,
    number: Option<String>,
    amount_minor: Option<i64>,
    currency: Option<String>,
    file_size: Option<i64>,
    file_mtime: Option<i64>,
    missing_since: Option<i64>,
}

impl DocumentRow {
    pub fn id(&self) -> i64 {
        self.id
    }

    pub fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            hash: row.get(1)?,
            path: row.get(2)?,
            source: row.get(3)?,
            received_at: row.get(4)?,
            status: row.get(5)?,
            sender: row.get(6)?,
            kind: row.get(7)?,
            tax_context: row.get(8)?,
            date: row.get(9)?,
            number: row.get(10)?,
            amount_minor: row.get(11)?,
            currency: row.get(12)?,
            file_size: row.get(13)?,
            file_mtime: row.get(14)?,
            missing_since: row.get(15)?,
        })
    }

    pub fn into_document(self, tags: BTreeSet<String>) -> Result<Document, IndexError> {
        let amount = match (self.amount_minor, self.currency) {
            (Some(minor_units), Some(currency)) => Some(Amount {
                minor_units,
                currency: currency_from_column(&currency)?,
            }),
            _ => None,
        };
        Ok(Document {
            id: DocumentId::new(self.id),
            hash: hash_from_column(&self.hash)?,
            path: PathBuf::from(self.path),
            source: DocumentSource::parse(&self.source)?,
            received_at: timestamp_from_column("document.received_at", self.received_at)?,
            status: DocumentStatus::parse(&self.status)?,
            fields: DocumentFields {
                sender: self.sender,
                kind: self.kind,
                tax_context: self.tax_context.map(TaxContext::from_short_name),
                date: self.date.as_deref().map(date_from_column).transpose()?,
                number: self.number,
                amount,
                tags,
            },
            stamp: FileStamp::from_columns(self.file_size, self.file_mtime)?,
            missing_since: self
                .missing_since
                .map(|seconds| timestamp_from_column("document.missing_since", seconds))
                .transpose()?,
        })
    }
}

impl Index {
    /// The document starts as `New`. A hash that is already indexed is
    /// `DuplicateHash` carrying the existing id.
    pub fn insert_document(&self, document: &NewDocument) -> Result<DocumentId, IndexError> {
        let path = document
            .path
            .to_str()
            .ok_or_else(|| IndexError::NonUtf8Path(document.path.clone()))?;
        let inserted = self.connection().execute(
            "INSERT INTO document (hash, path, source, received_at, status, file_size, file_mtime)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON CONFLICT (hash) DO NOTHING",
            params![
                document.hash.0.as_slice(),
                path,
                document.source.as_str(),
                document.received_at.timestamp(),
                DocumentStatus::New.as_str(),
                document.stamp.map(FileStamp::size_column),
                document.stamp.map(FileStamp::mtime_nanos),
            ],
        )?;
        if inserted == 0 {
            let existing = self.connection().query_row(
                "SELECT id FROM document WHERE hash = ?1",
                [document.hash.0.as_slice()],
                |row| row.get(0),
            )?;
            return Err(IndexError::DuplicateHash(DocumentId::new(existing)));
        }
        Ok(DocumentId::new(self.connection().last_insert_rowid()))
    }

    pub fn document_by_hash(&self, hash: &ContentHash) -> Result<Option<Document>, IndexError> {
        self.load_document(
            &format!("{DOCUMENT_COLUMNS} WHERE d.hash = ?1"),
            hash.0.as_slice(),
        )
    }

    /// `None` for a path that is only recorded as a copy (see
    /// `document_by_duplicate_path`) and for a missing document.
    pub fn document_by_path(&self, path: &Path) -> Result<Option<Document>, IndexError> {
        let path_text = path
            .to_str()
            .ok_or_else(|| IndexError::NonUtf8Path(path.to_path_buf()))?;
        self.load_document(
            &format!("{DOCUMENT_COLUMNS} WHERE d.path = ?1 AND d.missing_since IS NULL ORDER BY d.id LIMIT 1"),
            path_text,
        )
    }

    pub fn document(&self, id: DocumentId) -> Result<Option<Document>, IndexError> {
        self.load_document(&format!("{DOCUMENT_COLUMNS} WHERE d.id = ?1"), id.get())
    }

    /// Replaces the full text that `search_text` finds the document by.
    pub fn set_document_text(&mut self, id: DocumentId, text: &str) -> Result<(), IndexError> {
        let tx = self.conn_mut().transaction()?;
        require_document(&tx, id)?;
        tx.execute("DELETE FROM document_text WHERE rowid = ?1", [id.get()])?;
        tx.execute(
            "INSERT INTO document_text (rowid, body) VALUES (?1, ?2)",
            params![id.get(), text],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Replaces every field and the whole tag set. A field whose stored value
    /// changes gets `origin_of(field)`, a cleared one loses its origin, an
    /// unchanged one keeps the origin it has.
    pub fn set_document_fields(
        &mut self,
        id: DocumentId,
        fields: &DocumentFields,
        origin_of: impl Fn(Field) -> FieldOrigin,
    ) -> Result<(), IndexError> {
        let tx = self.conn_mut().transaction()?;
        let stored = tx
            .query_row(
                "SELECT sender_id, kind_id, tax_context_id, document_date, document_number,
                     amount_minor, currency
                 FROM document WHERE id = ?1",
                [id.get()],
                StoredFields::read,
            )
            .optional()?
            .ok_or(IndexError::DocumentNotFound(id))?;
        let sender_id = optional_name_id(&tx, NameTable::Sender, fields.sender.as_deref())?;
        let kind_id = optional_name_id(&tx, NameTable::Kind, fields.kind.as_deref())?;
        let tax_context_id = optional_tax_context_id(&tx, fields.tax_context.as_ref())?;
        let date = fields.date.map(|date| date.to_string());
        let amount_minor = fields.amount.as_ref().map(|amount| amount.minor_units);
        let currency = fields
            .amount
            .as_ref()
            .map(|amount| amount.currency.as_str());
        let changes = [
            (
                Field::Sender,
                stored.sender_id != sender_id,
                sender_id.is_some(),
            ),
            (Field::Kind, stored.kind_id != kind_id, kind_id.is_some()),
            (
                Field::TaxContext,
                stored.tax_context_id != tax_context_id,
                tax_context_id.is_some(),
            ),
            (Field::Date, stored.date != date, date.is_some()),
            (
                Field::Number,
                stored.number != fields.number,
                fields.number.is_some(),
            ),
            (
                Field::Amount,
                stored.amount_minor != amount_minor || stored.currency.as_deref() != currency,
                amount_minor.is_some(),
            ),
        ];
        tx.execute(
            "UPDATE document SET sender_id = ?2, kind_id = ?3, tax_context_id = ?4,
                 document_date = ?5, document_number = ?6, amount_minor = ?7, currency = ?8
             WHERE id = ?1",
            params![
                id.get(),
                sender_id,
                kind_id,
                tax_context_id,
                date,
                fields.number,
                amount_minor,
                currency,
            ],
        )?;
        for (field, changed, has_value) in changes {
            if changed {
                store_field_origin(&tx, id, field, has_value.then(|| origin_of(field)))?;
            }
        }
        tx.execute(
            "DELETE FROM document_tag WHERE document_id = ?1",
            [id.get()],
        )?;
        for tag in &fields.tags {
            let tag_id = name_id(&tx, NameTable::Tag, tag)?;
            tx.execute(
                "INSERT OR IGNORE INTO document_tag (document_id, tag_id) VALUES (?1, ?2)",
                [id.get(), tag_id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Fields without a stored origin read as `FieldOrigin::Unknown`.
    pub fn field_origins(&self, id: DocumentId) -> Result<FieldOrigins, IndexError> {
        let mut statement = self.connection().prepare(
            "SELECT field, origin, rule_id, confidence FROM document_field_origin
             WHERE document_id = ?1",
        )?;
        let rows = statement
            .query_map([id.get()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut stored = HashMap::new();
        for (field_name, origin, rule_id, confidence) in rows {
            let field = Field::ALL
                .into_iter()
                .find(|candidate| candidate.as_str() == field_name)
                .ok_or_else(|| corrupt("document_field_origin.field", &field_name))?;
            stored.insert(
                field,
                FieldOrigin::from_columns(&origin, rule_id, confidence)?,
            );
        }
        Ok(FieldOrigins::new(stored))
    }

    pub fn set_document_status(
        &self,
        id: DocumentId,
        status: DocumentStatus,
    ) -> Result<(), IndexError> {
        let changed = self.connection().execute(
            "UPDATE document SET status = ?2 WHERE id = ?1",
            params![id.get(), status.as_str()],
        )?;
        if changed == 0 {
            return Err(IndexError::DocumentNotFound(id));
        }
        Ok(())
    }

    /// The documents with `status` that are not missing, oldest first.
    pub fn documents_with_status(
        &self,
        status: DocumentStatus,
    ) -> Result<Vec<DocumentId>, IndexError> {
        let mut statement = self.connection().prepare(
            "SELECT id FROM document WHERE status = ?1 AND missing_since IS NULL ORDER BY id",
        )?;
        let ids = statement
            .query_map([status.as_str()], |row| row.get(0).map(DocumentId::new))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ids)
    }

    /// Best match first. The input is the user's words, not FTS5 syntax: every
    /// word is quoted, so all of them must match and operators are plain text.
    pub fn search_text(&self, words: &str, limit: usize) -> Result<Vec<DocumentId>, IndexError> {
        self.run_text_query(&match_query(words, false), limit)
    }

    /// Like [`Self::search_text`], but the last word only has to begin a word of
    /// the text, so a result is there while the user is still typing.
    pub fn search_text_prefix(
        &self,
        words: &str,
        limit: usize,
    ) -> Result<Vec<DocumentId>, IndexError> {
        self.run_text_query(&match_query(words, true), limit)
    }

    fn run_text_query(&self, query: &str, limit: usize) -> Result<Vec<DocumentId>, IndexError> {
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let mut statement = self.connection().prepare(
            "SELECT rowid FROM document_text WHERE document_text MATCH ?1
             ORDER BY rank LIMIT ?2",
        )?;
        let limit_rows = i64::try_from(limit).unwrap_or(i64::MAX);
        let ids = statement
            .query_map(params![query, limit_rows], |row| {
                row.get(0).map(DocumentId::new)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ids)
    }

    /// The full text `set_document_text` stored; `None` when there is none.
    pub fn document_text(&self, id: DocumentId) -> Result<Option<String>, IndexError> {
        let found = self
            .connection()
            .query_row(
                "SELECT body FROM document_text WHERE rowid = ?1",
                [id.get()],
                |row| row.get(0),
            )
            .optional()?;
        Ok(found)
    }

    fn load_document(
        &self,
        sql: &str,
        key: impl rusqlite::ToSql,
    ) -> Result<Option<Document>, IndexError> {
        let Some(row) = self
            .connection()
            .query_row(sql, [key], DocumentRow::read)
            .optional()?
        else {
            return Ok(None);
        };
        let tags = self.document_tags(row.id)?;
        row.into_document(tags).map(Some)
    }

    pub fn document_tags(&self, id: i64) -> Result<BTreeSet<String>, IndexError> {
        let mut statement = self.connection().prepare(
            "SELECT t.name FROM document_tag dt JOIN tag t ON t.id = dt.tag_id
             WHERE dt.document_id = ?1",
        )?;
        let tags = statement
            .query_map([id], |row| row.get(0))?
            .collect::<Result<BTreeSet<String>, _>>()?;
        Ok(tags)
    }
}

struct StoredFields {
    sender_id: Option<i64>,
    kind_id: Option<i64>,
    tax_context_id: Option<i64>,
    date: Option<String>,
    number: Option<String>,
    amount_minor: Option<i64>,
    currency: Option<String>,
}

impl StoredFields {
    fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            sender_id: row.get(0)?,
            kind_id: row.get(1)?,
            tax_context_id: row.get(2)?,
            date: row.get(3)?,
            number: row.get(4)?,
            amount_minor: row.get(5)?,
            currency: row.get(6)?,
        })
    }
}

/// `None` and `Unknown` both leave the field without a row.
fn store_field_origin(
    conn: &Connection,
    id: DocumentId,
    field: Field,
    origin: Option<FieldOrigin>,
) -> Result<(), IndexError> {
    let Some(kind) = origin.and_then(FieldOrigin::kind_column) else {
        conn.execute(
            "DELETE FROM document_field_origin WHERE document_id = ?1 AND field = ?2",
            params![id.get(), field.as_str()],
        )?;
        return Ok(());
    };
    let rule_id = origin.and_then(FieldOrigin::rule_column);
    let confidence = origin.and_then(FieldOrigin::confidence_column);
    conn.execute(
        "INSERT INTO document_field_origin (document_id, field, origin, rule_id, confidence)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (document_id, field) DO UPDATE SET
             origin = excluded.origin, rule_id = excluded.rule_id,
             confidence = excluded.confidence",
        params![id.get(), field.as_str(), kind, rule_id, confidence],
    )?;
    Ok(())
}

pub fn require_document(conn: &Connection, id: DocumentId) -> Result<(), IndexError> {
    let exists = conn
        .query_row("SELECT 1 FROM document WHERE id = ?1", [id.get()], |_| {
            Ok(())
        })
        .optional()?
        .is_some();
    if exists {
        Ok(())
    } else {
        Err(IndexError::DocumentNotFound(id))
    }
}

/// The words as an FTS5 query, each one quoted; with `prefix_last` the last one
/// carries the prefix star. Empty when there is no word.
fn match_query(words: &str, prefix_last: bool) -> String {
    let quoted: Vec<String> = words
        .split_whitespace()
        .map(|word| format!("\"{}\"", word.replace('"', "\"\"")))
        .collect();
    let last = quoted.len().saturating_sub(1);
    quoted
        .into_iter()
        .enumerate()
        .map(|(at, word)| {
            if prefix_last && at == last {
                format!("{word}*")
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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
    use crate::model::Currency;
    use crate::test_support::{encrypted_index, new_document, plain_index};

    fn indexed_with_text(index: &mut Index, seed: u8, text: &str) -> DocumentId {
        let id = index.insert_document(&new_document(seed)).unwrap();
        index.set_document_text(id, text).unwrap();
        id
    }

    #[test]
    fn inserted_document_round_trips_with_its_fields() {
        let (_dir, mut index) = encrypted_index();
        let new = new_document(1);
        let id = index.insert_document(&new).unwrap();
        let fields = DocumentFields {
            sender: Some("Company A".to_owned()),
            kind: Some("Invoice".to_owned()),
            tax_context: Some(TaxContext::named("company")),
            date: NaiveDate::from_ymd_opt(2026, 2, 28),
            number: Some("4711".to_owned()),
            amount: Some(Amount {
                minor_units: 12_345,
                currency: Currency::new("EUR").unwrap(),
            }),
            tags: BTreeSet::from(["services".to_owned(), "2026".to_owned()]),
        };
        index
            .set_document_fields(id, &fields, |_| FieldOrigin::Unknown)
            .unwrap();

        let expected = Document {
            id,
            hash: new.hash,
            path: new.path.clone(),
            source: new.source,
            received_at: new.received_at,
            status: DocumentStatus::New,
            fields,
            stamp: None,
            missing_since: None,
        };
        assert_eq!(
            index.document_by_hash(&new.hash).unwrap(),
            Some(expected.clone())
        );
        assert_eq!(index.document(id).unwrap(), Some(expected));
        assert_eq!(index.document(DocumentId::new(id.get() + 1)).unwrap(), None);
    }

    #[test]
    fn new_document_has_no_fields_and_status_can_change() {
        let (_dir, index) = plain_index();
        let id = index.insert_document(&new_document(1)).unwrap();
        assert_eq!(
            index.document(id).unwrap().unwrap().fields,
            DocumentFields::default()
        );

        index
            .set_document_status(id, DocumentStatus::Filed)
            .unwrap();

        assert_eq!(
            index.document(id).unwrap().unwrap().status,
            DocumentStatus::Filed
        );
    }

    #[test]
    fn second_insert_with_the_same_hash_reports_the_first_id() {
        let (_dir, index) = encrypted_index();
        let first = index.insert_document(&new_document(1)).unwrap();

        let err = index.insert_document(&new_document(1)).unwrap_err();

        assert!(
            matches!(err, IndexError::DuplicateHash(id) if id == first),
            "{err}"
        );
    }

    #[test]
    fn setters_report_an_unknown_document() {
        let (_dir, mut index) = encrypted_index();
        let missing = DocumentId::new(99);

        let status = index.set_document_status(missing, DocumentStatus::Filed);
        let text = index.set_document_text(missing, "x");
        let fields = index.set_document_fields(
            missing,
            &DocumentFields {
                sender: Some("Company A".to_owned()),
                ..DocumentFields::default()
            },
            |_| FieldOrigin::Unknown,
        );

        for result in [status, text, fields] {
            assert!(
                matches!(result, Err(IndexError::DocumentNotFound(id)) if id == missing),
                "{result:?}"
            );
        }
        let senders: i64 = index
            .connection()
            .query_row("SELECT count(*) FROM sender", [], |row| row.get(0))
            .unwrap();
        assert_eq!(senders, 0);
    }

    #[test]
    fn setting_fields_again_replaces_the_tags() {
        let (_dir, mut index) = encrypted_index();
        let id = index.insert_document(&new_document(1)).unwrap();
        let tagged = DocumentFields {
            tags: BTreeSet::from(["a".to_owned(), "b".to_owned()]),
            ..DocumentFields::default()
        };
        index
            .set_document_fields(id, &tagged, |_| FieldOrigin::Unknown)
            .unwrap();
        let only_b = DocumentFields {
            tags: BTreeSet::from(["b".to_owned()]),
            ..DocumentFields::default()
        };

        index
            .set_document_fields(id, &only_b, |_| FieldOrigin::Unknown)
            .unwrap();

        assert_eq!(index.document(id).unwrap().unwrap().fields, only_b);
    }

    #[test]
    fn full_text_is_found_by_word_number_and_several_words() {
        let (_dir, mut index) = encrypted_index();
        let id = indexed_with_text(&mut index, 1, "Rechnung Nr. 4711 von Company A");
        indexed_with_text(&mut index, 2, "Mahnung Nr. 12 von Agency B");

        assert_eq!(index.search_text("rechnung", 10).unwrap(), vec![id]);
        assert_eq!(index.search_text("4711", 10).unwrap(), vec![id]);
        assert_eq!(index.search_text("company  4711", 10).unwrap(), vec![id]);
        assert!(index.search_text("rechnung agency", 10).unwrap().is_empty());
        assert!(index.search_text("absent", 10).unwrap().is_empty());
        assert!(index.search_text("   ", 10).unwrap().is_empty());
        assert_eq!(index.search_text("nr", 1).unwrap().len(), 1);
    }

    #[test]
    fn the_prefix_search_finds_a_word_by_its_beginning() {
        let (_dir, mut index) = encrypted_index();
        let id = indexed_with_text(&mut index, 1, "Rechnung Nr. 4711 von Company A");
        indexed_with_text(&mut index, 2, "Mahnung Nr. 12 von Agency B");

        assert_eq!(index.search_text_prefix("rechn", 10).unwrap(), vec![id]);
        assert_eq!(
            index.search_text_prefix("company rech", 10).unwrap(),
            vec![id]
        );
        assert!(index.search_text("rechn", 10).unwrap().is_empty());
        assert!(index
            .search_text_prefix("rech agency", 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn the_prefix_search_folds_diacritics_and_quotes_its_input() {
        let (_dir, mut index) = encrypted_index();
        let id = indexed_with_text(&mut index, 1, "Gebühr für Januar");

        assert_eq!(index.search_text_prefix("gebu", 10).unwrap(), vec![id]);
        assert_eq!(index.search_text_prefix("Gebüh", 10).unwrap(), vec![id]);
        for input in [
            "\"",
            "AND",
            "col:x",
            "*",
            "-x",
            "\"unterminated",
            "NEAR(a b)",
        ] {
            index.search_text_prefix(input, 10).unwrap();
        }
    }

    #[test]
    fn the_prefix_search_of_nothing_finds_nothing() {
        let (_dir, mut index) = encrypted_index();
        indexed_with_text(&mut index, 1, "Rechnung");

        assert!(index.search_text_prefix("", 10).unwrap().is_empty());
        assert!(index.search_text_prefix("   ", 10).unwrap().is_empty());
    }

    #[test]
    fn full_text_folds_diacritics() {
        let (_dir, mut index) = encrypted_index();
        let id = indexed_with_text(&mut index, 1, "Gebühr für Januar");

        assert_eq!(index.search_text("gebuhr", 10).unwrap(), vec![id]);
        assert_eq!(index.search_text("Gebühr", 10).unwrap(), vec![id]);
    }

    #[test]
    fn hostile_search_input_is_plain_words_and_never_an_error() {
        let (_dir, mut index) = encrypted_index();
        indexed_with_text(&mut index, 1, "Rechnung and not x");

        for input in [
            "\"",
            "AND",
            "NOT x",
            "col:x",
            "*",
            "-",
            "-x",
            "\"unterminated",
            "( )",
            "NEAR(a b)",
        ] {
            index.search_text(input, 10).unwrap();
        }
    }

    #[test]
    fn the_stored_text_is_read_back() {
        let (_dir, mut index) = encrypted_index();
        let id = indexed_with_text(&mut index, 1, "Company A, Invoice 4711");
        let without = index.insert_document(&new_document(2)).unwrap();

        assert_eq!(
            index.document_text(id).unwrap().as_deref(),
            Some("Company A, Invoice 4711")
        );
        assert_eq!(index.document_text(without).unwrap(), None);
    }

    #[test]
    fn setting_the_text_again_replaces_it() {
        let (_dir, mut index) = encrypted_index();
        let id = indexed_with_text(&mut index, 1, "alpha");

        index.set_document_text(id, "beta").unwrap();

        assert!(index.search_text("alpha", 10).unwrap().is_empty());
        assert_eq!(index.search_text("beta", 10).unwrap(), vec![id]);
    }

    #[test]
    fn deleting_a_document_removes_its_tags_reviews_and_text() {
        let (_dir, mut index) = encrypted_index();
        let id = indexed_with_text(&mut index, 1, "Rechnung");
        index
            .set_document_fields(
                id,
                &DocumentFields {
                    tags: BTreeSet::from(["a".to_owned()]),
                    ..DocumentFields::default()
                },
                |_| FieldOrigin::Unknown,
            )
            .unwrap();
        index
            .add_review(id, crate::ReviewReason::NoMatchingRule)
            .unwrap();

        index
            .connection()
            .execute("DELETE FROM document WHERE id = ?1", [id.get()])
            .unwrap();

        let count = |table: &str| -> i64 {
            index
                .connection()
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap()
        };
        assert_eq!(count("document_tag"), 0);
        assert_eq!(count("review_entry"), 0);
        assert!(index.search_text("rechnung", 10).unwrap().is_empty());

        // The deleted id is reused; the old text must not find the new document.
        let reused = index.insert_document(&new_document(2)).unwrap();
        assert_eq!(reused, id);
        assert!(index.search_text("rechnung", 10).unwrap().is_empty());
    }

    fn all_fields() -> DocumentFields {
        DocumentFields {
            sender: Some("Company A".to_owned()),
            kind: Some("Invoice".to_owned()),
            tax_context: Some(TaxContext::named("company")),
            date: NaiveDate::from_ymd_opt(2026, 2, 28),
            number: Some("4711".to_owned()),
            amount: Some(Amount {
                minor_units: 12_345,
                currency: Currency::new("EUR").unwrap(),
            }),
            tags: BTreeSet::new(),
        }
    }

    fn all_origins(index: &Index, id: DocumentId) -> Vec<FieldOrigin> {
        let origins = index.field_origins(id).unwrap();
        Field::ALL
            .into_iter()
            .map(|field| origins.of(field))
            .collect()
    }

    #[test]
    fn a_fresh_document_reads_every_field_origin_as_unknown() {
        let (_dir, mut index) = encrypted_index();
        let id = index.insert_document(&new_document(1)).unwrap();

        assert_eq!(all_origins(&index, id), [FieldOrigin::Unknown; 6]);
        index
            .set_document_fields(id, &all_fields(), |_| FieldOrigin::Unknown)
            .unwrap();
        assert_eq!(all_origins(&index, id), [FieldOrigin::Unknown; 6]);
    }

    #[test]
    fn each_kind_of_origin_round_trips() {
        let (_dir, mut index) = encrypted_index();
        let rule = index
            .create_rule(&crate::test_support::rule_spec())
            .unwrap();
        let id = index.insert_document(&new_document(1)).unwrap();

        index
            .set_document_fields(id, &all_fields(), |field| match field {
                Field::Sender => FieldOrigin::Rule(Some(rule)),
                Field::Kind => FieldOrigin::Ai {
                    confidence_percent: 87,
                },
                Field::TaxContext => FieldOrigin::Manual,
                Field::Date => FieldOrigin::Ai {
                    confidence_percent: 0,
                },
                Field::Number => FieldOrigin::Ai {
                    confidence_percent: 100,
                },
                Field::Amount => FieldOrigin::Unknown,
            })
            .unwrap();

        assert_eq!(
            all_origins(&index, id),
            [
                FieldOrigin::Rule(Some(rule)),
                FieldOrigin::Ai {
                    confidence_percent: 87
                },
                FieldOrigin::Manual,
                FieldOrigin::Ai {
                    confidence_percent: 0
                },
                FieldOrigin::Ai {
                    confidence_percent: 100
                },
                FieldOrigin::Unknown,
            ]
        );
    }

    #[test]
    fn an_unchanged_field_keeps_its_origin_and_a_changed_one_takes_the_new_origin() {
        let (_dir, mut index) = encrypted_index();
        let id = index.insert_document(&new_document(1)).unwrap();
        index
            .set_document_fields(id, &all_fields(), |_| FieldOrigin::Manual)
            .unwrap();

        let mut edited = all_fields();
        edited.number = Some("4712".to_owned());
        edited.amount = Some(Amount {
            minor_units: 12_345,
            currency: Currency::new("USD").unwrap(),
        });
        index
            .set_document_fields(id, &edited, |_| FieldOrigin::Ai {
                confidence_percent: 90,
            })
            .unwrap();

        let ai = FieldOrigin::Ai {
            confidence_percent: 90,
        };
        assert_eq!(
            all_origins(&index, id),
            [
                FieldOrigin::Manual,
                FieldOrigin::Manual,
                FieldOrigin::Manual,
                FieldOrigin::Manual,
                ai,
                ai,
            ]
        );
    }

    #[test]
    fn a_changed_field_set_with_an_unknown_origin_loses_its_old_origin() {
        let (_dir, mut index) = encrypted_index();
        let id = index.insert_document(&new_document(1)).unwrap();
        index
            .set_document_fields(id, &all_fields(), |_| FieldOrigin::Manual)
            .unwrap();

        let mut edited = all_fields();
        edited.sender = Some("Company B".to_owned());
        index
            .set_document_fields(id, &edited, |_| FieldOrigin::Unknown)
            .unwrap();

        let origins = index.field_origins(id).unwrap();
        assert_eq!(origins.of(Field::Sender), FieldOrigin::Unknown);
        assert_eq!(origins.of(Field::Kind), FieldOrigin::Manual);
    }

    #[test]
    fn clearing_a_field_makes_its_origin_unknown() {
        let (_dir, mut index) = encrypted_index();
        let id = index.insert_document(&new_document(1)).unwrap();
        index
            .set_document_fields(id, &all_fields(), |_| FieldOrigin::Manual)
            .unwrap();

        let mut cleared = all_fields();
        cleared.date = None;
        cleared.amount = None;
        index
            .set_document_fields(id, &cleared, |_| FieldOrigin::Manual)
            .unwrap();

        let origins = index.field_origins(id).unwrap();
        assert_eq!(origins.of(Field::Date), FieldOrigin::Unknown);
        assert_eq!(origins.of(Field::Amount), FieldOrigin::Unknown);
        assert_eq!(origins.of(Field::Sender), FieldOrigin::Manual);
    }

    #[test]
    fn deleting_the_rule_leaves_a_rule_origin_without_a_rule() {
        let (_dir, mut index) = encrypted_index();
        let rule = index
            .create_rule(&crate::test_support::rule_spec())
            .unwrap();
        let id = index.insert_document(&new_document(1)).unwrap();
        index
            .set_document_fields(id, &all_fields(), |_| FieldOrigin::Rule(Some(rule)))
            .unwrap();

        assert!(index.delete_rule(rule).unwrap());

        assert_eq!(all_origins(&index, id), [FieldOrigin::Rule(None); 6]);
    }

    #[test]
    fn deleting_a_document_removes_its_field_origins() {
        let (_dir, mut index) = encrypted_index();
        let id = index.insert_document(&new_document(1)).unwrap();
        index
            .set_document_fields(id, &all_fields(), |_| FieldOrigin::Manual)
            .unwrap();

        index
            .connection()
            .execute("DELETE FROM document WHERE id = ?1", [id.get()])
            .unwrap();

        let remaining: i64 = index
            .connection()
            .query_row("SELECT count(*) FROM document_field_origin", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(remaining, 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_is_not_utf8_is_refused() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt as _;

        let (_dir, index) = encrypted_index();
        let mut document = new_document(1);
        document.path = PathBuf::from(OsStr::from_bytes(b"inbox/caf\xE9.pdf"));

        let err = index.insert_document(&document).unwrap_err();

        assert!(
            matches!(&err, IndexError::NonUtf8Path(path) if *path == document.path),
            "{err}"
        );
    }

    #[test]
    fn names_are_trimmed_and_compared_without_case() {
        let (_dir, mut index) = encrypted_index();
        for (seed, name) in [(1, "  Company A "), (2, "Company A"), (3, "company a")] {
            let id = index.insert_document(&new_document(seed)).unwrap();
            let fields = DocumentFields {
                sender: Some(name.to_owned()),
                ..DocumentFields::default()
            };
            index
                .set_document_fields(id, &fields, |_| FieldOrigin::Unknown)
                .unwrap();
        }

        let senders: i64 = index
            .connection()
            .query_row("SELECT count(*) FROM sender", [], |row| row.get(0))
            .unwrap();
        assert_eq!(senders, 1);
    }

    #[test]
    fn names_with_non_ascii_letters_are_one_sender_and_keep_the_first_spelling() {
        let (_dir, mut index) = encrypted_index();
        let spellings = ["Ärztekammer", "ÄRZTEKAMMER", " ärztekammer "];
        for (seed, name) in (1..).zip(spellings) {
            let id = index.insert_document(&new_document(seed)).unwrap();
            let fields = DocumentFields {
                sender: Some(name.to_owned()),
                tags: BTreeSet::from([name.to_owned()]),
                ..DocumentFields::default()
            };
            index
                .set_document_fields(id, &fields, |_| FieldOrigin::Unknown)
                .unwrap();
        }

        let stored = |table: &str| -> Vec<String> {
            let mut statement = index
                .connection()
                .prepare(&format!("SELECT name FROM {table}"))
                .unwrap();
            statement
                .query_map([], |row| row.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        assert_eq!(stored("sender"), vec!["Ärztekammer".to_owned()]);
        assert_eq!(stored("tag"), vec!["Ärztekammer".to_owned()]);
        let last = index.document(DocumentId::new(3)).unwrap().unwrap();
        assert_eq!(last.fields.sender.as_deref(), Some("Ärztekammer"));
    }

    #[test]
    fn an_empty_name_is_refused() {
        let (_dir, mut index) = encrypted_index();
        let id = index.insert_document(&new_document(1)).unwrap();

        let err = index
            .set_document_fields(
                id,
                &DocumentFields {
                    sender: Some("   ".to_owned()),
                    ..DocumentFields::default()
                },
                |_| FieldOrigin::Unknown,
            )
            .unwrap_err();

        assert!(
            matches!(err, IndexError::EmptyName { what: "sender" }),
            "{err}"
        );
    }

    #[test]
    fn an_unknown_stored_status_is_an_error_not_a_panic() {
        let (_dir, index) = encrypted_index();
        let id = index.insert_document(&new_document(1)).unwrap();
        index
            .connection()
            .execute("UPDATE document SET status = 'archived'", [])
            .unwrap();

        let err = index.document(id).unwrap_err();

        assert!(
            matches!(&err, IndexError::CorruptValue { column: "document.status", value } if value == "archived"),
            "{err}"
        );
    }
}
