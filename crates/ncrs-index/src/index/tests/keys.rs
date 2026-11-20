use super::super::*;
use crate::error::KeyStoreError;
use crate::key_store::InMemoryKeyStore;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

pub(in crate::index::tests) const TABLE_A: &str = "CREATE TABLE a (id INTEGER);";
pub(in crate::index::tests) const TABLE_B: &str = "CREATE TABLE b (id INTEGER);";

pub(in crate::index::tests) fn db_in(dir: &TempDir) -> PathBuf {
    dir.path().join("index.db")
}

pub(in crate::index::tests) fn create_encrypted_db(
    path: &Path,
    store: &InMemoryKeyStore,
) -> RecoveryKey {
    let (index, recovery_key) = Index::create_encrypted(path, store).unwrap();
    drop(index);
    recovery_key
}

pub(in crate::index::tests) fn table_exists(index: &Index, name: &str) -> bool {
    index
        .conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name = ?1",
            [name],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
        == 1
}

pub(in crate::index::tests) fn user_version(index: &Index) -> u32 {
    index
        .conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap()
}

#[test]
fn new_encrypted_database_reopens_with_the_same_store() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let store = InMemoryKeyStore::empty();

    create_encrypted_db(&path, &store);
    let key = store.snapshot().unwrap();
    assert_eq!(key.len(), 32);

    let reopened = Index::open(&path, &store).unwrap();
    assert!(reopened.is_encrypted());
    assert_eq!(store.snapshot().unwrap(), key);
}

#[test]
fn encrypted_file_does_not_start_with_the_sqlite_header() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    create_encrypted_db(&path, &InMemoryKeyStore::empty());

    let bytes = fs::read(&path).unwrap();
    assert!(bytes.len() >= PLAINTEXT_HEADER.len());
    assert!(!bytes.starts_with(PLAINTEXT_HEADER));
}

#[test]
fn plain_database_opens_without_touching_the_store() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    drop(Index::create_plain(&path).unwrap());
    assert!(fs::read(&path).unwrap().starts_with(PLAINTEXT_HEADER));

    let store = InMemoryKeyStore::empty();
    let index = Index::open(&path, &store).unwrap();

    assert!(!index.is_encrypted());
    assert_eq!(store.snapshot(), None);
}

#[test]
fn create_on_an_existing_path_fails_and_leaves_the_file() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    create_encrypted_db(&path, &InMemoryKeyStore::empty());
    let file_before = fs::read(&path).unwrap();

    let store = InMemoryKeyStore::empty();
    let encrypted_err = Index::create_encrypted(&path, &store).unwrap_err();
    let plain_err = Index::create_plain(&path).unwrap_err();

    assert!(
        matches!(encrypted_err, IndexError::AlreadyExists),
        "{encrypted_err}"
    );
    assert!(
        matches!(plain_err, IndexError::AlreadyExists),
        "{plain_err}"
    );
    assert_eq!(fs::read(&path).unwrap(), file_before);
    assert_eq!(store.snapshot(), None);
}

#[test]
fn create_encrypted_reuses_an_entry_left_by_a_deleted_database() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let store = InMemoryKeyStore::empty();
    let first = create_encrypted_db(&path, &store);
    let entry = store.snapshot().unwrap();
    fs::remove_file(&path).unwrap();

    let (index, second) = Index::create_encrypted(&path, &store).unwrap();
    drop(index);

    assert_eq!(second.to_string(), first.to_string());
    assert_eq!(store.snapshot().unwrap(), entry);
    assert!(Index::open(&path, &store).unwrap().is_encrypted());
}

#[test]
fn create_encrypted_with_an_entry_of_wrong_length_changes_nothing() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let store = InMemoryKeyStore::with_entry(vec![9; 16]);

    let err = Index::create_encrypted(&path, &store).unwrap_err();

    assert!(
        matches!(err, IndexError::InvalidKeyLength { found: 16 }),
        "{err}"
    );
    assert_eq!(store.snapshot().unwrap(), vec![9; 16]);
    assert!(!path.exists());
}

#[test]
fn open_on_a_missing_file_does_not_create_it() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let store = InMemoryKeyStore::empty();

    let err = Index::open(&path, &store).unwrap_err();

    assert!(matches!(err, IndexError::NotFound), "{err}");
    assert!(!path.exists());
    assert_eq!(store.snapshot(), None);
}

#[test]
fn other_key_is_rejected_and_nothing_changes() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    create_encrypted_db(&path, &InMemoryKeyStore::empty());
    let file_before = fs::read(&path).unwrap();

    let other = InMemoryKeyStore::with_entry(vec![7; 32]);
    let err = Index::open(&path, &other).unwrap_err();

    assert!(matches!(err, IndexError::WrongKey), "{err}");
    assert_eq!(fs::read(&path).unwrap(), file_before);
    assert_eq!(other.snapshot().unwrap(), vec![7; 32]);
}

#[test]
fn missing_key_for_existing_database_is_reported_without_making_one() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    create_encrypted_db(&path, &InMemoryKeyStore::empty());
    let file_before = fs::read(&path).unwrap();

    let empty = InMemoryKeyStore::empty();
    let err = Index::open(&path, &empty).unwrap_err();

    assert!(matches!(err, IndexError::KeyMissing), "{err}");
    assert_eq!(fs::read(&path).unwrap(), file_before);
    assert_eq!(empty.snapshot(), None);
}

#[test]
fn entry_of_wrong_length_is_rejected_and_kept() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    create_encrypted_db(&path, &InMemoryKeyStore::empty());
    let file_before = fs::read(&path).unwrap();

    let short = InMemoryKeyStore::with_entry(vec![1; 16]);
    let err = Index::open(&path, &short).unwrap_err();

    assert!(
        matches!(err, IndexError::InvalidKeyLength { found: 16 }),
        "{err}"
    );
    assert_eq!(fs::read(&path).unwrap(), file_before);
    assert_eq!(short.snapshot().unwrap(), vec![1; 16]);
}

#[test]
fn restore_key_with_the_right_key_makes_open_work_again() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let recovery_key = create_encrypted_db(&path, &InMemoryKeyStore::empty());
    let file_before = fs::read(&path).unwrap();

    let fresh_store = InMemoryKeyStore::empty();
    Index::restore_key(&path, &recovery_key, &fresh_store).unwrap();

    assert_eq!(
        fresh_store.snapshot().unwrap(),
        recovery_key.key().as_bytes()
    );
    assert!(Index::open(&path, &fresh_store).unwrap().is_encrypted());
    assert_eq!(fs::read(&path).unwrap(), file_before);
}

#[test]
fn restore_key_with_a_wrong_key_stores_nothing() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    create_encrypted_db(&path, &InMemoryKeyStore::empty());
    let file_before = fs::read(&path).unwrap();
    let wrong = RecoveryKey::from_str_for_test(&[3; 32]);

    let store = InMemoryKeyStore::empty();
    let err = Index::restore_key(&path, &wrong, &store).unwrap_err();

    assert!(matches!(err, IndexError::WrongKey), "{err}");
    assert_eq!(store.snapshot(), None);
    assert_eq!(fs::read(&path).unwrap(), file_before);
}

#[test]
fn restore_key_refuses_to_replace_an_entry() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let recovery_key = create_encrypted_db(&path, &InMemoryKeyStore::empty());

    let store = InMemoryKeyStore::with_entry(vec![4; 32]);
    let err = Index::restore_key(&path, &recovery_key, &store).unwrap_err();

    assert!(matches!(err, IndexError::KeyStoreNotEmpty), "{err}");
    assert_eq!(store.snapshot().unwrap(), vec![4; 32]);
}

#[test]
fn replace_key_with_the_right_key_replaces_a_different_entry() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let recovery_key = create_encrypted_db(&path, &InMemoryKeyStore::empty());
    let file_before = fs::read(&path).unwrap();

    let store = InMemoryKeyStore::with_entry(vec![4; 32]);
    Index::replace_key(&path, &recovery_key, &store).unwrap();

    assert_eq!(store.snapshot().unwrap(), recovery_key.key().as_bytes());
    assert!(Index::open(&path, &store).unwrap().is_encrypted());
    assert_eq!(fs::read(&path).unwrap(), file_before);
}

#[test]
fn replace_key_with_a_wrong_key_keeps_the_entry() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    create_encrypted_db(&path, &InMemoryKeyStore::empty());
    let wrong = RecoveryKey::from_str_for_test(&[3; 32]);

    let store = InMemoryKeyStore::with_entry(vec![4; 32]);
    let err = Index::replace_key(&path, &wrong, &store).unwrap_err();

    assert!(matches!(err, IndexError::WrongKey), "{err}");
    assert_eq!(store.snapshot().unwrap(), vec![4; 32]);
}

#[test]
fn replace_key_on_a_plain_file_is_refused_and_keeps_the_entry() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    drop(Index::create_plain(&path).unwrap());
    let some_key = RecoveryKey::from_str_for_test(&[3; 32]);

    let store = InMemoryKeyStore::with_entry(vec![4; 32]);
    let err = Index::replace_key(&path, &some_key, &store).unwrap_err();

    assert!(matches!(err, IndexError::NotEncrypted), "{err}");
    assert_eq!(store.snapshot().unwrap(), vec![4; 32]);
}

#[test]
fn replace_key_on_a_missing_file_creates_nothing() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let some_key = RecoveryKey::from_str_for_test(&[3; 32]);

    let store = InMemoryKeyStore::with_entry(vec![4; 32]);
    let err = Index::replace_key(&path, &some_key, &store).unwrap_err();

    assert!(matches!(err, IndexError::NotFound), "{err}");
    assert!(!path.exists());
    assert_eq!(store.snapshot().unwrap(), vec![4; 32]);
}

#[test]
fn store_new_refuses_to_overwrite() {
    let store = InMemoryKeyStore::with_entry(vec![1; 32]);
    assert_eq!(store.store_new(&[2; 32]), Err(KeyStoreError::AlreadyExists));
    assert_eq!(store.snapshot().unwrap(), vec![1; 32]);
}

#[test]
fn migrations_run_once_and_set_user_version() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let store = InMemoryKeyStore::empty();
    let steps = [TABLE_A, TABLE_B];

    let (first, _) = Index::create_encrypted_with_migrations(&path, &store, &steps).unwrap();
    assert_eq!(user_version(&first), 2);
    drop(first);

    // A repeat of "CREATE TABLE a" would fail, so success proves nothing ran twice.
    let second = Index::open_with_migrations(&path, &store, &steps).unwrap();
    assert_eq!(user_version(&second), 2);
    assert!(table_exists(&second, "a") && table_exists(&second, "b"));
}

#[test]
fn later_migrations_are_applied_on_top_of_earlier_ones() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let store = InMemoryKeyStore::empty();
    drop(Index::create_encrypted_with_migrations(&path, &store, &[TABLE_A]).unwrap());

    let index = Index::open_with_migrations(&path, &store, &[TABLE_A, TABLE_B]).unwrap();

    assert_eq!(user_version(&index), 2);
    assert!(table_exists(&index, "b"));
}

#[test]
fn failing_migration_leaves_version_and_schema_as_before() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let store = InMemoryKeyStore::empty();
    drop(Index::create_encrypted_with_migrations(&path, &store, &[TABLE_A]).unwrap());

    let broken = [TABLE_A, "CREATE TABLE b (id INTEGER); THIS IS NOT SQL;"];
    let err = Index::open_with_migrations(&path, &store, &broken).unwrap_err();
    assert!(
        matches!(
            err,
            IndexError::MigrationFailed {
                from: 1,
                step: 2,
                ..
            }
        ),
        "{err}"
    );

    let index = Index::open_with_migrations(&path, &store, &[TABLE_A]).unwrap();
    assert_eq!(user_version(&index), 1);
    assert!(table_exists(&index, "a"));
    assert!(!table_exists(&index, "b"));
}

#[test]
fn a_failed_creation_leaves_no_file_and_the_next_try_works() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let store = InMemoryKeyStore::empty();

    let broken = ["CREATE TABLE b (id INTEGER); THIS IS NOT SQL;"];
    Index::create_encrypted_with_migrations(&path, &store, &broken).unwrap_err();
    assert!(!path.exists());

    Index::create_encrypted_with_migrations(&path, &store, &[TABLE_A]).unwrap();
    std::fs::remove_file(&path).unwrap();
    Index::create_plain_with_migrations(&path, &broken).unwrap_err();
    assert!(!path.exists());
    Index::create_plain_with_migrations(&path, &[TABLE_A]).unwrap();
}
