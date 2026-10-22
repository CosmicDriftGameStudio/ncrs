use rusqlite::Connection;

use crate::error::IndexError;

/// Schema steps, applied in order. The domain schema arrives with the first
/// real step; the runner is exercised by tests until then.
pub const MIGRATIONS: &[&str] = &[];

/// Step `n` (1-based) runs in one transaction together with the bump of
/// `user_version` to `n`, so a failing step leaves the previous state intact.
pub fn run_migrations(conn: &mut Connection, steps: &[&str]) -> Result<(), IndexError> {
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
        tx.pragma_update(
            None,
            "user_version",
            u32::try_from(index + 1).unwrap_or(u32::MAX),
        )?;
        tx.commit()?;
    }
    Ok(())
}
