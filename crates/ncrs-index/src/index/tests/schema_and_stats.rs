use super::super::*;
use super::keys::*;
use crate::key_store::InMemoryKeyStore;
use tempfile::TempDir;

#[test]
fn empty_a1_database_gains_the_document_schema_on_open() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let store = InMemoryKeyStore::empty();
    drop(Index::create_encrypted_with_migrations(&path, &store, &[]).unwrap());

    let index = Index::open(&path, &store).unwrap();

    assert_eq!(
        user_version(&index),
        u32::try_from(MIGRATIONS.len()).unwrap()
    );
    for table in ["document", "document_text", "rule", "review_entry", "tag"] {
        assert!(table_exists(&index, table), "{table}");
    }
}

#[test]
fn foreign_keys_are_enforced_on_every_kind_of_connection() {
    let dir = TempDir::new().unwrap();
    let store = InMemoryKeyStore::empty();
    let encrypted_path = dir.path().join("encrypted.db");
    let plain_path = dir.path().join("plain.db");
    let (created, _) = Index::create_encrypted(&encrypted_path, &store).unwrap();
    let plain = Index::create_plain(&plain_path).unwrap();
    let reopened = Index::open(&encrypted_path, &store).unwrap();

    for index in [&created, &plain, &reopened] {
        let on: i64 = index
            .connection()
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .unwrap();
        assert_eq!(on, 1);
    }
}

#[test]
fn a_step_that_leaves_a_dangling_reference_is_rolled_back() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let store = InMemoryKeyStore::empty();
    drop(Index::create_encrypted_with_migrations(&path, &store, &[TABLE_A]).unwrap());
    let dangling = "CREATE TABLE p (id INTEGER PRIMARY KEY);
                    CREATE TABLE c (p_id INTEGER REFERENCES p (id));
                    INSERT INTO c VALUES (5);";

    let err = Index::open_with_migrations(&path, &store, &[TABLE_A, dangling]).unwrap_err();
    assert!(
        matches!(&err, IndexError::MigrationFailed { from: 1, step: 2, reason } if reason.contains("foreign key")),
        "{err}"
    );
    assert!(err.to_string().contains("left unchanged"), "{err}");

    let index = Index::open_with_migrations(&path, &store, &[TABLE_A]).unwrap();
    assert_eq!(user_version(&index), 1);
    assert!(!table_exists(&index, "p") && !table_exists(&index, "c"));
}

#[test]
fn database_from_a_newer_version_is_refused() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let store = InMemoryKeyStore::empty();
    let steps = [TABLE_A, TABLE_B];
    drop(Index::create_encrypted_with_migrations(&path, &store, &steps).unwrap());

    let err = Index::open_with_migrations(&path, &store, &steps[..1]).unwrap_err();
    assert!(
        matches!(
            err,
            IndexError::SchemaTooNew {
                found: 2,
                supported: 1
            }
        ),
        "{err}"
    );
}

