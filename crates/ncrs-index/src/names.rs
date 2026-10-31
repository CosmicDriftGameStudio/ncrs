use rusqlite::Connection;

use crate::error::IndexError;
use crate::kinds::KindCatalog;

/// The lookup tables whose rows are addressed by name.
#[derive(Debug, Clone, Copy)]
pub enum NameTable {
    Sender,
    Kind,
    Tag,
}

impl NameTable {
    fn table(self) -> &'static str {
        match self {
            Self::Sender => "sender",
            Self::Kind => "document_kind",
            Self::Tag => "tag",
        }
    }

    fn what(self) -> &'static str {
        match self {
            Self::Sender => "sender",
            Self::Kind => "kind",
            Self::Tag => "tag",
        }
    }
}

/// Looks the name up, inserting it when new. Surrounding whitespace is
/// dropped; a kind is expected as its stored form already. Names are matched by
/// their lower-cased key, computed here because SQLite's `NOCASE` folds ASCII only; `name` keeps the spelling seen first.
pub fn name_id(conn: &Connection, table: NameTable, raw_name: &str) -> Result<i64, IndexError> {
    let name = raw_name.trim().to_owned();
    if name.is_empty() {
        return Err(IndexError::EmptyName { what: table.what() });
    }
    let key = name.to_lowercase();
    let table_name = table.table();
    conn.execute(
        &format!(
            "INSERT INTO {table_name} (name, name_key) VALUES (?1, ?2)
             ON CONFLICT (name_key) DO NOTHING"
        ),
        [name.as_str(), key.as_str()],
    )?;
    Ok(conn.query_row(
        &format!("SELECT id FROM {table_name} WHERE name_key = ?1"),
        [key.as_str()],
        |row| row.get(0),
    )?)
}

/// Like `optional_name_id` for a kind, which is looked up by its ID when known.
pub fn optional_kind_id(
    conn: &Connection,
    kinds: &KindCatalog,
    kind: Option<&str>,
) -> Result<Option<i64>, IndexError> {
    kind.map(|present| name_id(conn, NameTable::Kind, &kinds.stored_kind(present)))
        .transpose()
}

pub fn optional_name_id(
    conn: &Connection,
    table: NameTable,
    name: Option<&str>,
) -> Result<Option<i64>, IndexError> {
    name.map(|present| name_id(conn, table, present))
        .transpose()
}
