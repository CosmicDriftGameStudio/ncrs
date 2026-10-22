use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension as _};

use crate::error::IndexError;
use crate::index::Index;
use crate::model::{
    currency_from_column, date_from_column, hash_from_column, timestamp_from_column, Amount,
    ContentHash, Document, DocumentFields, DocumentId, DocumentSource, DocumentStatus, FileStamp,
    NewDocument, TaxContext,
};
use crate::names::{name_id, optional_name_id, NameTable};

const DOCUMENT_COLUMNS: &str = "SELECT d.id, d.hash, d.path, d.source, d.received_at, d.status,
        s.name, k.name, d.tax_context, d.document_date, d.document_number,
        d.amount_minor, d.currency, d.file_size, d.file_mtime, d.missing_since
    FROM document d
    LEFT JOIN sender s ON s.id = d.sender_id
    LEFT JOIN document_kind k ON k.id = d.kind_id";

/// A row as stored, before its text columns are parsed into the model types.
struct DocumentRow {
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
    fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
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

    fn into_document(self, tags: BTreeSet<String>) -> Result<Document, IndexError> {
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
                tax_context: self
                    .tax_context
                    .as_deref()
                    .map(|value| TaxContext::parse("document.tax_context", value))
                    .transpose()?,
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

    /// Replaces every field and the whole tag set.
    pub fn set_document_fields(
        &mut self,
        id: DocumentId,
        fields: &DocumentFields,
    ) -> Result<(), IndexError> {
        let tx = self.conn_mut().transaction()?;
        let sender_id = optional_name_id(&tx, NameTable::Sender, fields.sender.as_deref())?;
        let kind_id = optional_name_id(&tx, NameTable::Kind, fields.kind.as_deref())?;
        let changed = tx.execute(
            "UPDATE document SET sender_id = ?2, kind_id = ?3, tax_context = ?4,
                 document_date = ?5, document_number = ?6, amount_minor = ?7, currency = ?8
             WHERE id = ?1",
            params![
                id.get(),
                sender_id,
                kind_id,
                fields.tax_context.map(TaxContext::as_str),
                fields.date.map(|date| date.to_string()),
                fields.number,
                fields.amount.as_ref().map(|amount| amount.minor_units),
                fields
                    .amount
                    .as_ref()
                    .map(|amount| amount.currency.as_str()),
            ],
        )?;
        if changed == 0 {
            return Err(IndexError::DocumentNotFound(id));
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

    /// Best match first. The input is the user's words, not FTS5 syntax: every
    /// word is quoted, so all of them must match and operators are plain text.
    pub fn search_text(&self, words: &str, limit: usize) -> Result<Vec<DocumentId>, IndexError> {
        let query = words
            .split_whitespace()
            .map(|word| format!("\"{}\"", word.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" ");
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

    fn document_tags(&self, id: i64) -> Result<BTreeSet<String>, IndexError> {
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
            tax_context: Some(TaxContext::Company),
            date: NaiveDate::from_ymd_opt(2026, 2, 28),
            number: Some("4711".to_owned()),
            amount: Some(Amount {
                minor_units: 12_345,
                currency: Currency::new("EUR").unwrap(),
            }),
            tags: BTreeSet::from(["services".to_owned(), "2026".to_owned()]),
        };
        index.set_document_fields(id, &fields).unwrap();

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
        index.set_document_fields(id, &tagged).unwrap();
        let only_b = DocumentFields {
            tags: BTreeSet::from(["b".to_owned()]),
            ..DocumentFields::default()
        };

        index.set_document_fields(id, &only_b).unwrap();

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
            index.set_document_fields(id, &fields).unwrap();
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
            index.set_document_fields(id, &fields).unwrap();
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
