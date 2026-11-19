use super::super::*;
use super::keys::*;
use crate::error::KeyStoreError;
use crate::key_store::InMemoryKeyStore;
use crate::model::DocumentId;
use crate::test_support::new_document;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

/// Refuses to store, as a credential store that has gone away does.
struct StoreThatRefusesToSave;

impl KeyStore for StoreThatRefusesToSave {
    fn load(&self) -> Result<Option<Vec<u8>>, KeyStoreError> {
        Ok(None)
    }

    fn store_new(&self, _key: &[u8]) -> Result<(), KeyStoreError> {
        Err(KeyStoreError::Unavailable("refused".to_owned()))
    }

    fn replace(&self, _key: &[u8]) -> Result<(), KeyStoreError> {
        Err(KeyStoreError::Unavailable("refused".to_owned()))
    }

    fn delete(&self) -> Result<(), KeyStoreError> {
        Ok(())
    }
}

fn plain_db_with_a_document(path: &Path) -> DocumentId {
    let mut index = Index::create_plain(path).unwrap();
    let id = index.insert_document(&new_document(1)).unwrap();
    index
        .set_document_text(id, "invoice for the garden fence")
        .unwrap();
    id
}

fn files_beside(dir: &TempDir) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn encrypting_a_plain_index_keeps_its_rows_version_and_text_search() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let id = plain_db_with_a_document(&path);
    let store = InMemoryKeyStore::empty();

    let recovery_key = Index::encrypt_plain(&path, &store).unwrap();

    assert!(!Index::file_is_plain(&path).unwrap());
    let index = Index::open(&path, &store).unwrap();
    assert!(index.is_encrypted());
    assert_eq!(
        user_version(&index),
        u32::try_from(MIGRATIONS.len()).unwrap()
    );
    assert_eq!(
        index.document(id).unwrap().unwrap().path,
        PathBuf::from("inbox/scan-1.pdf")
    );
    assert_eq!(index.search_text("garden", 10).unwrap(), vec![id]);
    let stored = store.snapshot().unwrap();
    assert_eq!(recovery_key.key().as_bytes(), stored.as_slice());
    assert_eq!(files_beside(&dir), vec!["index.db".to_owned()]);
}

#[test]
fn encrypting_with_a_key_already_in_the_store_reuses_it_and_keeps_it() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    plain_db_with_a_document(&path);
    let store = InMemoryKeyStore::with_entry(vec![7; 32]);

    Index::encrypt_plain(&path, &store).unwrap();

    assert_eq!(store.snapshot(), Some(vec![7; 32]));
    assert!(Index::open(&path, &store).unwrap().is_encrypted());
}

#[test]
fn a_store_that_refuses_to_save_leaves_the_plain_file_and_no_copy() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    plain_db_with_a_document(&path);
    let before = fs::read(&path).unwrap();

    let err = Index::encrypt_plain(&path, &StoreThatRefusesToSave).unwrap_err();

    assert!(matches!(err, IndexError::KeyStore(_)), "{err}");
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(Index::file_is_plain(&path).unwrap());
    assert_eq!(files_beside(&dir), vec!["index.db".to_owned()]);
}

#[test]
fn an_old_copy_from_a_stopped_run_is_replaced() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    plain_db_with_a_document(&path);
    fs::write(dir.path().join("index.db.encrypting"), b"left over").unwrap();
    let store = InMemoryKeyStore::empty();

    Index::encrypt_plain(&path, &store).unwrap();

    assert_eq!(files_beside(&dir), vec!["index.db".to_owned()]);
    assert!(Index::open(&path, &store).unwrap().is_encrypted());
}

#[test]
fn an_encrypted_index_is_not_encrypted_again() {
    let dir = TempDir::new().unwrap();
    let path = db_in(&dir);
    let store = InMemoryKeyStore::empty();
    create_encrypted_db(&path, &store);

    let err = Index::encrypt_plain(&path, &store).unwrap_err();

    assert!(matches!(err, IndexError::NotPlain), "{err}");
}

#[test]
fn a_missing_file_is_not_created_by_encrypting() {
    let dir = TempDir::new().unwrap();
    let err = Index::encrypt_plain(&db_in(&dir), &InMemoryKeyStore::empty()).unwrap_err();

    assert!(matches!(err, IndexError::NotFound), "{err}");
    assert!(files_beside(&dir).is_empty());
}

#[test]
fn foreign_keys_are_back_on_after_a_failed_migration() {
    let conn_result = {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        let failed = crate::migrations::run_migrations(&mut conn, &[TABLE_A, "NOT SQL"]);
        let on: i64 = conn
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .unwrap();
        (failed.is_err(), on)
    };
    assert_eq!(conn_result, (true, 1));
}
