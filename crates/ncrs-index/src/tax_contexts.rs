use rusqlite::{Connection, OptionalExtension as _};

use crate::error::IndexError;
use crate::index::Index;
use crate::model::{TaxContext, TaxContextEntry, TaxContextId};

/// The row that stands for "no context"; it is never listed.
const NO_CONTEXT_ID: i64 = 0;

/// Case-insensitive on the short name. The sentinel's short name is not a
/// context a caller can name.
pub fn tax_context_id(conn: &Connection, context: &TaxContext) -> Result<i64, IndexError> {
    let TaxContext::Named(short_name) = context else {
        return Ok(NO_CONTEXT_ID);
    };
    conn.query_row(
        "SELECT id FROM tax_context WHERE short_name = ?1 AND id <> ?2",
        rusqlite::params![short_name, NO_CONTEXT_ID],
        |row| row.get(0),
    )
    .optional()?
    .ok_or_else(|| IndexError::UnknownTaxContext(short_name.clone()))
}

pub fn optional_tax_context_id(
    conn: &Connection,
    context: Option<&TaxContext>,
) -> Result<Option<i64>, IndexError> {
    context.map(|named| tax_context_id(conn, named)).transpose()
}

impl Index {
    /// Ordered by name, without the "no context" row.
    pub fn tax_contexts(&self) -> Result<Vec<TaxContextEntry>, IndexError> {
        let mut statement = self.connection().prepare(
            "SELECT id, name, short_name FROM tax_context WHERE id <> ?1
             ORDER BY name COLLATE NOCASE, id",
        )?;
        let entries = statement
            .query_map([NO_CONTEXT_ID], |row| {
                Ok(TaxContextEntry {
                    id: TaxContextId::new(row.get(0)?),
                    name: row.get(1)?,
                    short_name: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(entries)
    }
}
