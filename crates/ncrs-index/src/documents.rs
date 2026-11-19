use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension as _};

use crate::error::IndexError;
use crate::index::Index;
use crate::model::{
    corrupt, currency_from_column, date_from_column, hash_from_column, timestamp_from_column,
    Amount, ContentHash, Document, DocumentFields, DocumentId, DocumentSource, DocumentStatus,
    Field, FieldOrigin, FieldOrigins, FileStamp, NewDocument,
};
use crate::names::{name_id, optional_kind_id, optional_name_id, NameTable};

pub const DOCUMENT_COLUMNS: &str = "SELECT d.id, d.hash, d.path, d.source, d.received_at, d.status,
        s.name, k.name, d.document_date, d.document_number,
        d.amount_minor, d.currency, d.file_size, d.file_mtime, d.missing_since, d.recipient
    FROM document d
    LEFT JOIN sender s ON s.id = d.sender_id
    LEFT JOIN document_kind k ON k.id = d.kind_id";

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
    date: Option<String>,
    number: Option<String>,
    amount_minor: Option<i64>,
    currency: Option<String>,
    file_size: Option<i64>,
    file_mtime: Option<i64>,
    missing_since: Option<i64>,
    recipient: Option<String>,
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
            date: row.get(8)?,
            number: row.get(9)?,
            amount_minor: row.get(10)?,
            currency: row.get(11)?,
            file_size: row.get(12)?,
            file_mtime: row.get(13)?,
            missing_since: row.get(14)?,
            recipient: row.get(15)?,
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
                recipient: self.recipient,
                kind: self.kind,
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
            &format!("{DOCUMENT_COLUMNS} WHERE d.path = ?1 AND d.origin_kind = 'file' AND d.missing_since IS NULL ORDER BY d.id LIMIT 1"),
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
        let kinds = Arc::clone(self.kinds());
        let tx = self.conn_mut().transaction()?;
        let stored = tx
            .query_row(
                "SELECT sender_id, kind_id, document_date, document_number,
                     amount_minor, currency, recipient
                 FROM document WHERE id = ?1",
                [id.get()],
                StoredFields::read,
            )
            .optional()?
            .ok_or(IndexError::DocumentNotFound(id))?;
        let sender_id = optional_name_id(&tx, NameTable::Sender, fields.sender.as_deref())?;
        let kind_id = optional_kind_id(&tx, &kinds, fields.kind.as_deref())?;
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
            (
                Field::Recipient,
                stored.recipient != fields.recipient,
                fields.recipient.is_some(),
            ),
            (Field::Kind, stored.kind_id != kind_id, kind_id.is_some()),
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
            "UPDATE document SET sender_id = ?2, kind_id = ?3,
                 document_date = ?4, document_number = ?5, amount_minor = ?6, currency = ?7,
                 recipient = ?8
             WHERE id = ?1",
            params![
                id.get(),
                sender_id,
                kind_id,
                date,
                fields.number,
                amount_minor,
                currency,
                fields.recipient,
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
        for (tags, automatic) in [(fields.own_tags(), 0), (fields.automatic_tags(&kinds), 1)] {
            for tag in &tags {
                let tag_id = name_id(&tx, NameTable::Tag, tag)?;
                tx.execute(
                    "INSERT OR IGNORE INTO document_tag (document_id, tag_id, automatic)
                     VALUES (?1, ?2, ?3)",
                    [id.get(), tag_id, automatic],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Fields without a stored origin read as `FieldOrigin::Unknown`.
    pub fn field_origins(&self, id: DocumentId) -> Result<FieldOrigins, IndexError> {
        let mut statement = self.connection().prepare(
            "SELECT field, origin, rule_id, confidence
             FROM document_field_origin
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

    /// Best match first, documents whose file is gone left out. The input is
    /// the user's words, not FTS5 syntax: every word is quoted, so all of them
    /// must match and operators are plain text.
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
            "SELECT document_text.rowid FROM document_text
             JOIN document d ON d.id = document_text.rowid
             WHERE document_text MATCH ?1 AND d.missing_since IS NULL
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

    /// A short piece of the text of each document around where `words` match,
    /// the matched words between [`SNIPPET_MARK_START`] and
    /// [`SNIPPET_MARK_END`]. The match is the one of [`Self::search_text_prefix`],
    /// so the marks follow the tokenizer. A document the words do not match has
    /// no entry.
    pub fn text_snippets(
        &self,
        ids: &[DocumentId],
        words: &str,
    ) -> Result<Vec<(DocumentId, String)>, IndexError> {
        let query = match_query(words, true);
        if query.is_empty() || ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut snippets_by_rowid = HashMap::new();
        for chunk in ids.chunks(SNIPPET_IDS_PER_STATEMENT) {
            let marks = vec!["?"; chunk.len()].join(", ");
            let mut statement = self.connection().prepare(&format!(
                "SELECT rowid, snippet(document_text, 0, ?, ?, '…', ?) FROM document_text
                 WHERE document_text MATCH ? AND rowid IN ({marks})"
            ))?;
            let mut values = vec![
                Value::Text(SNIPPET_MARK_START.to_owned()),
                Value::Text(SNIPPET_MARK_END.to_owned()),
                Value::Integer(SNIPPET_WORDS),
                Value::Text(query.clone()),
            ];
            values.extend(chunk.iter().map(|id| Value::Integer(id.get())));
            let rows = statement.query_map(params_from_iter(values), |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (rowid, snippet) = row?;
                snippets_by_rowid.insert(rowid, snippet);
            }
        }
        Ok(ids
            .iter()
            .filter_map(|id| Some((*id, snippets_by_rowid.remove(&id.get())?)))
            .collect())
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
             WHERE dt.document_id = ?1 AND dt.automatic = 0",
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
    date: Option<String>,
    number: Option<String>,
    amount_minor: Option<i64>,
    currency: Option<String>,
    recipient: Option<String>,
}

impl StoredFields {
    fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            sender_id: row.get(0)?,
            kind_id: row.get(1)?,
            date: row.get(2)?,
            number: row.get(3)?,
            amount_minor: row.get(4)?,
            currency: row.get(5)?,
            recipient: row.get(6)?,
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
    let Some(kind) = origin.as_ref().and_then(FieldOrigin::kind_column) else {
        conn.execute(
            "DELETE FROM document_field_origin WHERE document_id = ?1 AND field = ?2",
            params![id.get(), field.as_str()],
        )?;
        return Ok(());
    };
    let rule_id = origin.as_ref().and_then(FieldOrigin::rule_column);
    let confidence = origin.as_ref().and_then(FieldOrigin::confidence_column);
    conn.execute(
        "INSERT INTO document_field_origin
             (document_id, field, origin, rule_id, confidence)
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

/// Control characters as marks: extracted text practically never holds them,
/// and a stray one only shifts a highlight.
pub const SNIPPET_MARK_START: &str = "\u{2}";
pub const SNIPPET_MARK_END: &str = "\u{3}";

/// Words around the match in a snippet.
const SNIPPET_WORDS: i64 = 12;
/// Well below SQLite's limit of bound values per statement.
const SNIPPET_IDS_PER_STATEMENT: usize = 500;

/// The words as an FTS5 query, each one quoted; with `prefix_last` the last one
/// carries the prefix star. Empty when there is no word.
pub fn match_query(words: &str, prefix_last: bool) -> String {
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
mod tests;
