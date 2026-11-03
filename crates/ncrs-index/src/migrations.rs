use rusqlite::Connection;

use crate::error::IndexError;

/// Schema steps, applied in order. Append-only: a step is never edited once a
/// release has shipped it, and a release that adds steps adds a fixture built
/// with it (`tests/fixtures`).
pub const MIGRATIONS: &[&str] = &[
    include_str!("migrations/001_documents.sql"),
    include_str!("migrations/002_locations.sql"),
    include_str!("migrations/003_location_profile.sql"),
    include_str!("migrations/004_rule_evidence.sql"),
    include_str!("migrations/005_discarded_rule_suggestions.sql"),
    include_str!("migrations/006_reorganisation_plan.sql"),
    include_str!("migrations/007_execution_journal.sql"),
    include_str!("migrations/008_saved_queries.sql"),
    include_str!("migrations/009_excluded_path.sql"),
    include_str!("migrations/010_field_origin.sql"),
    include_str!("migrations/011_rule_conditions.sql"),
    include_str!("migrations/012_sender_alias.sql"),
    include_str!("migrations/013_rule_order.sql"),
    include_str!("migrations/014_rule_suggestion_kind.sql"),
    include_str!("migrations/015_sender_alias_per_sender.sql"),
    include_str!("migrations/016_tax_context.sql"),
    include_str!("migrations/017_tax_context_conditions.sql"),
    include_str!("migrations/018_rule_origin.sql"),
    include_str!("migrations/019_saved_query_sender.sql"),
    include_str!("migrations/020_tags_instead_of_tax_contexts.sql"),
    include_str!("migrations/021_file_list_tag.sql"),
    include_str!("migrations/022_fields_read_version.sql"),
    include_str!("migrations/023_named_runs.sql"),
];

/// All pending steps run in one transaction together with the bump of
/// `user_version`, so a failing step leaves the file at its old version with
/// its old content. The error names the version found and the failing step.
///
/// Foreign keys are off while steps run and switched on afterwards, also when
/// a step fails: a step that rebuilds a table drops it, and with enforcement on
/// that drop would fire `ON DELETE CASCADE` and silently delete the rows that
/// reference it. The bundled SQLite defaults to on, so it is switched off
/// explicitly. The pragma is a no-op inside a transaction, so it stays outside.
pub fn run_migrations(conn: &mut Connection, steps: &[&str]) -> Result<(), IndexError> {
    conn.pragma_update(None, "foreign_keys", false)?;
    let applied: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let supported = u32::try_from(steps.len()).unwrap_or(u32::MAX);
    let migrated = if applied > supported {
        Err(IndexError::SchemaTooNew {
            found: applied,
            supported,
        })
    } else {
        apply_pending_steps(conn, steps, applied, supported)
    };
    conn.pragma_update(None, "foreign_keys", true)?;
    migrated
}

fn apply_pending_steps(
    conn: &mut Connection,
    steps: &[&str],
    applied: u32,
    supported: u32,
) -> Result<(), IndexError> {
    if applied == supported {
        return Ok(());
    }
    let failed_at = |step: u32, reason: String| IndexError::MigrationFailed {
        from: applied,
        step,
        reason,
    };
    let tx = conn.transaction()?;
    for (index, sql) in steps.iter().enumerate().skip(applied as usize) {
        let step = u32::try_from(index + 1).unwrap_or(u32::MAX);
        tx.execute_batch(sql)
            .map_err(|error| failed_at(step, error.to_string()))?;
        let violated = tx
            .prepare("PRAGMA foreign_key_check")
            .and_then(|mut check| check.exists([]))
            .map_err(|error| failed_at(step, error.to_string()))?;
        if violated {
            return Err(failed_at(step, "a foreign key check failed".to_owned()));
        }
    }
    tx.pragma_update(None, "user_version", supported)?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn table_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name = ?1",
            [name],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
            == 1
    }

    fn version(conn: &Connection) -> u32 {
        conn.pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn a_failing_second_pending_step_undoes_the_first() {
        let mut conn = Connection::open_in_memory().unwrap();

        let err = run_migrations(
            &mut conn,
            &[
                "CREATE TABLE first (id INTEGER);",
                "CREATE TABLE first (id INTEGER);",
            ],
        )
        .unwrap_err();

        assert!(
            matches!(
                err,
                IndexError::MigrationFailed {
                    from: 0,
                    step: 2,
                    ..
                }
            ),
            "{err}"
        );
        assert!(!table_exists(&conn, "first"));
        assert_eq!(version(&conn), 0);
    }

    #[test]
    fn foreign_keys_stay_enforced_after_a_failed_or_refused_migration() {
        let foreign_keys = |conn: &Connection| -> i64 {
            conn.pragma_query_value(None, "foreign_keys", |row| row.get(0))
                .unwrap()
        };
        let mut failing = Connection::open_in_memory().unwrap();
        run_migrations(&mut failing, &["CREATE TABLE t (id INTEGER);", "NOT SQL;"]).unwrap_err();
        assert_eq!(foreign_keys(&failing), 1);

        let mut too_new = Connection::open_in_memory().unwrap();
        run_migrations(&mut too_new, &["CREATE TABLE t (id INTEGER);"]).unwrap();
        run_migrations(&mut too_new, &[]).unwrap_err();
        assert_eq!(foreign_keys(&too_new), 1);
    }

    #[test]
    fn pending_steps_apply_together_and_set_the_version_once() {
        let mut conn = Connection::open_in_memory().unwrap();

        run_migrations(
            &mut conn,
            &[
                "CREATE TABLE a (id INTEGER);",
                "CREATE TABLE b (id INTEGER);",
            ],
        )
        .unwrap();

        assert!(table_exists(&conn, "a") && table_exists(&conn, "b"));
        assert_eq!(version(&conn), 2);
    }

    #[test]
    fn batches_written_before_named_runs_keep_their_items_and_read_as_moves() {
        let mut conn = Connection::open_in_memory().unwrap();
        let (_, before) = MIGRATIONS.split_last().unwrap();
        run_migrations(&mut conn, before).unwrap();
        conn.execute_batch(
            "INSERT INTO execution_batch (kind, started_at, finished_at) VALUES ('filing', 5, 6);
             INSERT INTO execution_item (batch_id, seq, from_path, to_path, hash, duplicate, state)
             VALUES (1, 0, '/a', '/b', x'00', 0, 'moved');",
        )
        .unwrap();

        run_migrations(&mut conn, MIGRATIONS).unwrap();

        let (kind, name, transfer, finished): (String, Option<String>, String, Option<i64>) = conn
            .query_row(
                "SELECT kind, name, transfer, finished_at FROM execution_batch WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            (kind.as_str(), name, transfer.as_str(), finished),
            ("filing", None, "move", Some(6))
        );
        let items: i64 = conn
            .query_row("SELECT count(*) FROM execution_item", [], |row| row.get(0))
            .unwrap();
        assert_eq!(items, 1);
        conn.execute(
            "INSERT INTO execution_batch (kind, started_at, transfer) VALUES ('hermes', 7, 'trash')",
            [],
        )
        .unwrap();
    }
}
