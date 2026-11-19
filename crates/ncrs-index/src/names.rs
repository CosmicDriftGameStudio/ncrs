use rusqlite::{params, Connection, OptionalExtension};

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

pub use ncrs_index_types::name_key;

/// Looks the name up, inserting it when new. Surrounding whitespace is
/// dropped; a kind is expected as its stored form already. Names are matched by
/// their lower-cased key, computed here because SQLite's `NOCASE` folds ASCII only; `name` keeps the spelling seen first.
pub fn name_id(conn: &Connection, table: NameTable, raw_name: &str) -> Result<i64, IndexError> {
    let name = raw_name.trim().to_owned();
    if name.is_empty() {
        return Err(IndexError::EmptyName { what: table.what() });
    }
    let key = name_key(&name);
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

/// Brings the keys of rows written before names were composed to the key of
/// today. A row whose composed key already belongs to another row is merged
/// into that one: what pointed at it points at the other, and what would then
/// exist twice stays with the old row, which then stays too.
pub fn fold_name_keys(conn: &mut Connection) -> Result<(), IndexError> {
    let tx = conn.transaction()?;
    for table in [NameTable::Sender, NameTable::Kind, NameTable::Tag] {
        let table_name = table.table();
        let exists: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [table_name],
            |row| row.get(0),
        )?;
        if !exists {
            continue;
        }
        let stale: Vec<(i64, String)> = {
            let mut statement = tx.prepare(&format!("SELECT id, name_key FROM {table_name}"))?;
            let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect::<Result<Vec<(i64, String)>, _>>()?
        }
        .into_iter()
        .filter(|(_, key)| name_key(key) != *key)
        .collect();
        if stale.is_empty() {
            continue;
        }
        let references = references_to(&tx, table_name)?;
        for (old, key) in stale {
            let folded = name_key(&key);
            let existing: Option<i64> = tx
                .query_row(
                    &format!("SELECT id FROM {table_name} WHERE name_key = ?1"),
                    [folded.as_str()],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(keeper) = existing else {
                tx.execute(
                    &format!("UPDATE {table_name} SET name_key = ?1 WHERE id = ?2"),
                    params![folded, old],
                )?;
                continue;
            };
            let mut left_over = false;
            for (referencing, column) in &references {
                tx.execute(
                    &format!(
                        "UPDATE OR IGNORE {referencing} SET {column} = ?1 WHERE {column} = ?2"
                    ),
                    params![keeper, old],
                )?;
                let remaining: i64 = tx.query_row(
                    &format!("SELECT COUNT(*) FROM {referencing} WHERE {column} = ?1"),
                    [old],
                    |row| row.get(0),
                )?;
                left_over |= remaining > 0;
            }
            if !left_over {
                tx.execute(&format!("DELETE FROM {table_name} WHERE id = ?1"), [old])?;
            }
        }
    }
    tx.commit()?;
    Ok(())
}

/// The tables and columns that point at a row of `table`.
fn references_to(conn: &Connection, table: &str) -> Result<Vec<(String, String)>, IndexError> {
    let tables: Vec<String> = {
        let mut statement = conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table'")?;
        let rows = statement.query_map([], |row| row.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    let mut found = Vec::new();
    for referencing in tables {
        let mut statement = conn.prepare(&format!(
            "SELECT \"table\", \"from\" FROM pragma_foreign_key_list('{referencing}')"
        ))?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (target, column) = row?;
            if target == table {
                found.push((referencing.clone(), column));
            }
        }
    }
    Ok(found)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const NFD: &str = "Mu\u{308}ller GmbH";
    const NFC: &str = "Müller GmbH";

    fn schema() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        conn.execute_batch(
            "CREATE TABLE sender (id INTEGER PRIMARY KEY, name TEXT NOT NULL, name_key TEXT NOT NULL UNIQUE);
             CREATE TABLE document_kind (id INTEGER PRIMARY KEY, name TEXT NOT NULL, name_key TEXT NOT NULL UNIQUE);
             CREATE TABLE tag (id INTEGER PRIMARY KEY, name TEXT NOT NULL, name_key TEXT NOT NULL UNIQUE);
             CREATE TABLE document (id INTEGER PRIMARY KEY, sender_id INTEGER REFERENCES sender (id));
             CREATE TABLE sender_alias (
                 sender_id INTEGER NOT NULL REFERENCES sender (id) ON DELETE CASCADE,
                 alias TEXT NOT NULL,
                 UNIQUE (sender_id, alias));",
        )
        .unwrap();
        conn
    }

    #[test]
    fn decomposed_and_composed_spellings_are_one_name() {
        assert_eq!(name_key(NFD), name_key(NFC));
        assert_eq!(name_key("  Müller GmbH "), "müller gmbh");
    }

    #[test]
    fn a_name_found_with_either_spelling_is_one_row() {
        let conn = schema();
        let first = name_id(&conn, NameTable::Sender, NFD).unwrap();
        let second = name_id(&conn, NameTable::Sender, NFC).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn old_rows_with_both_spellings_are_merged() {
        let mut conn = schema();
        conn.execute_batch(&format!(
            "INSERT INTO sender VALUES (1, '{NFC}', '{}');
             INSERT INTO sender VALUES (2, '{NFD}', '{}');
             INSERT INTO document VALUES (10, 2);
             INSERT INTO sender_alias VALUES (2, 'MG');",
            NFC.to_lowercase(),
            NFD.to_lowercase(),
        ))
        .unwrap();
        fold_name_keys(&mut conn).unwrap();
        let senders: i64 = conn
            .query_row("SELECT COUNT(*) FROM sender", [], |row| row.get(0))
            .unwrap();
        let owner: i64 = conn
            .query_row("SELECT sender_id FROM document WHERE id = 10", [], |row| {
                row.get(0)
            })
            .unwrap();
        let alias_owner: i64 = conn
            .query_row("SELECT sender_id FROM sender_alias", [], |row| row.get(0))
            .unwrap();
        assert_eq!((senders, owner, alias_owner), (1, 1, 1));
    }

    #[test]
    fn an_old_row_without_a_twin_gets_the_composed_key() {
        let mut conn = schema();
        conn.execute(
            "INSERT INTO sender VALUES (1, ?1, ?2)",
            params![NFD, NFD.to_lowercase()],
        )
        .unwrap();
        fold_name_keys(&mut conn).unwrap();
        assert_eq!(name_id(&conn, NameTable::Sender, NFC).unwrap(), 1);
    }
}
