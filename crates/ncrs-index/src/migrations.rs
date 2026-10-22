use rusqlite::Connection;

use crate::error::IndexError;

/// Schema steps, applied in order.
pub const MIGRATIONS: &[&str] = &[include_str!("migrations/001_documents.sql")];

/// Step `n` (1-based) runs in one transaction together with the bump of
/// `user_version` to `n`, so a failing step leaves the previous state intact.
///
/// Foreign keys are off while steps run and switched on afterwards: a step that
/// rebuilds a table drops it, and with enforcement on that drop would fire
/// `ON DELETE CASCADE` and silently delete the rows that reference it. The
/// bundled SQLite defaults to on, so it is switched off explicitly.
pub fn run_migrations(conn: &mut Connection, steps: &[&str]) -> Result<(), IndexError> {
    conn.pragma_update(None, "foreign_keys", false)?;
    let applied: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let supported = u32::try_from(steps.len()).unwrap_or(u32::MAX);
    if applied > supported {
        return Err(IndexError::SchemaTooNew {
            found: applied,
            supported,
        });
    }
    for (index, sql) in steps.iter().enumerate().skip(applied as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        if tx.prepare("PRAGMA foreign_key_check")?.exists([])? {
            return Err(IndexError::ForeignKeyViolation);
        }
        tx.pragma_update(
            None,
            "user_version",
            u32::try_from(index + 1).unwrap_or(u32::MAX),
        )?;
        tx.commit()?;
    }
    conn.pragma_update(None, "foreign_keys", true)?;
    Ok(())
}
