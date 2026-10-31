use std::fs::File;
use std::io::{ErrorKind, Read as _};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, ErrorCode};

use crate::error::IndexError;
use crate::key::{IndexKey, RecoveryKey};
use crate::key_store::{InMemoryKeyStore, KeyStore};
use crate::migrations::{run_migrations, MIGRATIONS};
use crate::model::IndexStatistics;

/// The FTS5 table `document_text` keeps its data in these shadow tables.
const FULL_TEXT_TABLES: [&str; 5] = [
    "document_text_data",
    "document_text_idx",
    "document_text_content",
    "document_text_docsize",
    "document_text_config",
];

const PLAINTEXT_HEADER: &[u8; 16] = b"SQLite format 3\0";

#[derive(Debug)]
pub struct Index {
    conn: Connection,
    encrypted: bool,
}

impl Index {
    /// Creates an encrypted database. The key is made, stored, and read back
    /// before the file exists, so a store that loses writes is noticed while
    /// nothing depends on it. An existing entry is never replaced.
    pub fn create_encrypted(
        db_path: &Path,
        store: &dyn KeyStore,
    ) -> Result<(Self, RecoveryKey), IndexError> {
        Self::create_encrypted_with_migrations(db_path, store, MIGRATIONS)
    }

    pub fn create_plain(db_path: &Path) -> Result<Self, IndexError> {
        Self::create_plain_with_migrations(db_path, MIGRATIONS)
    }

    /// Whether the file at `db_path` is an unencrypted index, judged by its
    /// header alone; no key is needed.
    pub fn file_is_plain(db_path: &Path) -> Result<bool, IndexError> {
        starts_with_plaintext_header(db_path)
    }

    /// Never creates a file. A file with the plain SQLite header is opened
    /// without touching the store; anything else needs the stored key.
    pub fn open(db_path: &Path, store: &dyn KeyStore) -> Result<Self, IndexError> {
        Self::open_with_migrations(db_path, store, MIGRATIONS)
    }

    /// Puts a key the user typed back into an empty credential store. It is
    /// only saved after it has opened the database.
    pub fn restore_key(
        db_path: &Path,
        recovery_key: &RecoveryKey,
        store: &dyn KeyStore,
    ) -> Result<(), IndexError> {
        require_existing_file(db_path)?;
        if store.load()?.is_some() {
            return Err(IndexError::KeyStoreNotEmpty);
        }
        let key = recovery_key.key();
        let conn = Connection::open(db_path)?;
        unlock(&conn, key)?;
        drop(conn);
        store_and_verify(store, key)
    }

    /// Overwrites the stored key with a recovery key the user typed. The store
    /// is only touched after that key has opened the database.
    pub fn replace_key(
        db_path: &Path,
        recovery_key: &RecoveryKey,
        store: &dyn KeyStore,
    ) -> Result<(), IndexError> {
        require_existing_file(db_path)?;
        if starts_with_plaintext_header(db_path)? {
            return Err(IndexError::NotEncrypted);
        }
        let key = recovery_key.key();
        let conn = Connection::open(db_path)?;
        unlock(&conn, key)?;
        drop(conn);
        store.replace(key.as_bytes())?;
        let read_back = store
            .load()?
            .ok_or(IndexError::KeyStoreVerificationFailed)?;
        if read_back == key.as_bytes() {
            Ok(())
        } else {
            Err(IndexError::KeyStoreVerificationFailed)
        }
    }

    /// Encrypts a plain index in place. The data is exported into a copy next
    /// to the file, the copy is opened with the key, the key goes into the
    /// store, and only then does the copy replace the plain file. A failure
    /// before that leaves the plain file as it was; a key made for this run is
    /// taken out of the store again.
    ///
    /// The caller must have closed every connection to `db_path`: on macOS a
    /// rename over an open file succeeds and the old handle then writes into
    /// the replaced file.
    pub fn encrypt_plain(db_path: &Path, store: &dyn KeyStore) -> Result<RecoveryKey, IndexError> {
        require_existing_file(db_path)?;
        if !starts_with_plaintext_header(db_path)? {
            return Err(IndexError::NotPlain);
        }
        let copy_path = path_with_suffix(db_path, ENCRYPTING_SUFFIX)?;
        remove_file_if_present(&copy_path)?;
        let (key, entry_is_new) = match store.load()? {
            Some(stored) => (IndexKey::from_stored(&stored)?, false),
            None => (IndexKey::generate()?, true),
        };
        let swapped = export_check_and_swap(db_path, &copy_path, &key, store, entry_is_new);
        if swapped.is_err() {
            let _copy_may_not_exist = std::fs::remove_file(&copy_path);
        }
        swapped.map(|()| key.recovery_key())
    }

    pub fn is_encrypted(&self) -> bool {
        self.encrypted
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Cheap counts for a status display; reads no document or review rows.
    pub fn statistics(&self) -> Result<IndexStatistics, IndexError> {
        let conn = self.connection();
        let schema_version = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        let document_count = conn.query_row("SELECT COUNT(*) FROM document", [], |row| {
            row.get::<_, i64>(0).map(i64::unsigned_abs)
        })?;
        let open_reviews = conn.query_row(
            "SELECT COUNT(*) FROM review_entry WHERE resolved_at IS NULL",
            [],
            |row| row.get::<_, i64>(0).map(i64::unsigned_abs),
        )?;
        let full_text_bytes = conn.query_row(
            &format!(
                "SELECT COALESCE(SUM(pgsize), 0) FROM dbstat WHERE name IN ({})",
                FULL_TEXT_TABLES.map(|name| format!("'{name}'")).join(", ")
            ),
            [],
            |row| row.get::<_, i64>(0).map(i64::unsigned_abs),
        )?;
        Ok(IndexStatistics {
            schema_version,
            document_count,
            open_reviews,
            full_text_bytes,
        })
    }

    pub(crate) fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    fn create_encrypted_with_migrations(
        db_path: &Path,
        store: &dyn KeyStore,
        migrations: &[&str],
    ) -> Result<(Self, RecoveryKey), IndexError> {
        require_no_file(db_path)?;
        let key = key_for_new_database(store)?;
        let mut conn = Connection::open(db_path)?;
        unlock(&conn, &key)?;
        write_header_and_migrate(&mut conn, migrations)?;
        let recovery_key = key.recovery_key();
        Ok((
            Self {
                conn,
                encrypted: true,
            },
            recovery_key,
        ))
    }

    fn create_plain_with_migrations(
        db_path: &Path,
        migrations: &[&str],
    ) -> Result<Self, IndexError> {
        require_no_file(db_path)?;
        let mut conn = Connection::open(db_path)?;
        write_header_and_migrate(&mut conn, migrations)?;
        Ok(Self {
            conn,
            encrypted: false,
        })
    }

    fn open_with_migrations(
        db_path: &Path,
        store: &dyn KeyStore,
        migrations: &[&str],
    ) -> Result<Self, IndexError> {
        require_existing_file(db_path)?;
        let encrypted = !starts_with_plaintext_header(db_path)?;
        let mut conn = Connection::open(db_path)?;
        if encrypted {
            // A key is never made for a file that exists: a new key could not
            // open it, and the data would be gone for good.
            let stored = store.load()?.ok_or(IndexError::KeyMissing)?;
            unlock(&conn, &IndexKey::from_stored(&stored)?)?;
        }
        run_migrations(&mut conn, migrations)?;
        Ok(Self { conn, encrypted })
    }
}

fn require_existing_file(db_path: &Path) -> Result<(), IndexError> {
    if db_path.try_exists().map_err(IndexError::Io)? {
        Ok(())
    } else {
        Err(IndexError::NotFound)
    }
}

fn require_no_file(db_path: &Path) -> Result<(), IndexError> {
    if db_path.try_exists().map_err(IndexError::Io)? {
        Err(IndexError::AlreadyExists)
    } else {
        Ok(())
    }
}

/// A file shorter than the header, such as an empty one left by a crash, is
/// not plain SQLite either and goes down the encrypted path.
fn starts_with_plaintext_header(db_path: &Path) -> Result<bool, IndexError> {
    let mut header = Vec::with_capacity(PLAINTEXT_HEADER.len());
    File::open(db_path)
        .and_then(|file| {
            file.take(PLAINTEXT_HEADER.len() as u64)
                .read_to_end(&mut header)
        })
        .map_err(IndexError::Io)?;
    Ok(header == PLAINTEXT_HEADER)
}

/// A deleted database, or a stop between saving the key and creating the file,
/// leaves an entry behind. The user has already saved its recovery key, and an
/// old database moved back into place opens with it, so it is reused.
fn key_for_new_database(store: &dyn KeyStore) -> Result<IndexKey, IndexError> {
    if let Some(stored) = store.load()? {
        return IndexKey::from_stored(&stored);
    }
    let key = IndexKey::generate()?;
    store_and_verify(store, &key)?;
    Ok(key)
}

fn store_and_verify(store: &dyn KeyStore, key: &IndexKey) -> Result<(), IndexError> {
    store.store_new(key.as_bytes())?;
    let read_back = store
        .load()?
        .ok_or(IndexError::KeyStoreVerificationFailed)?;
    if read_back == key.as_bytes() {
        Ok(())
    } else {
        Err(IndexError::KeyStoreVerificationFailed)
    }
}

/// The copy `encrypt_plain` writes; a leftover one is from a stopped run.
const ENCRYPTING_SUFFIX: &str = ".encrypting";

/// Files SQLite keeps beside the database; none may outlive a replaced one.
const SIDECAR_SUFFIXES: [&str; 3] = ["-wal", "-shm", "-journal"];

fn path_with_suffix(db_path: &Path, suffix: &str) -> Result<PathBuf, IndexError> {
    let mut name = db_path
        .file_name()
        .ok_or_else(|| IndexError::NonUtf8Path(db_path.to_path_buf()))?
        .to_os_string();
    name.push(suffix);
    Ok(db_path.with_file_name(name))
}

fn remove_file_if_present(path: &Path) -> Result<(), IndexError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(IndexError::Io(err)),
    }
}

fn export_check_and_swap(
    db_path: &Path,
    copy_path: &Path,
    key: &IndexKey,
    store: &dyn KeyStore,
    entry_is_new: bool,
) -> Result<(), IndexError> {
    export_encrypted_copy(db_path, copy_path, key)?;
    check_copy_opens(copy_path, key)?;
    if entry_is_new {
        if let Err(err) = store_and_verify(store, key) {
            let _entry_may_not_exist = store.delete();
            return Err(err);
        }
    }
    if let Err(err) = replace_with_copy(db_path, copy_path) {
        if entry_is_new {
            let _entry_may_not_exist = store.delete();
        }
        return Err(err);
    }
    sync_parent_directory(db_path);
    Ok(())
}

/// The key is a bound parameter, so it appears in no SQL text and no error.
fn export_encrypted_copy(
    db_path: &Path,
    copy_path: &Path,
    key: &IndexKey,
) -> Result<(), IndexError> {
    let copy_text = copy_path
        .to_str()
        .ok_or_else(|| IndexError::NonUtf8Path(copy_path.to_path_buf()))?;
    let conn = Connection::open(db_path)?;
    // The export copies tables one by one, so rows may not be checked against
    // tables that are not there yet.
    conn.pragma_update(None, "foreign_keys", false)?;
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    let user_version: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    conn.execute(
        "ATTACH DATABASE ?1 AS encrypted KEY ?2",
        (copy_text, raw_key_text(key)),
    )?;
    conn.execute_batch("PRAGMA encrypted.cipher_compatibility = 4;")?;
    conn.query_row("SELECT sqlcipher_export('encrypted')", [], |_| Ok(()))?;
    conn.execute_batch(&format!("PRAGMA encrypted.user_version = {user_version};"))?;
    conn.execute_batch("DETACH DATABASE encrypted;")?;
    conn.close().map_err(|(_, err)| err)?;
    Ok(())
}

fn check_copy_opens(copy_path: &Path, key: &IndexKey) -> Result<(), IndexError> {
    let holding_key = InMemoryKeyStore::with_entry(key.as_bytes().to_vec());
    let copy = Index::open(copy_path, &holding_key)?;
    if copy.is_encrypted() {
        Ok(())
    } else {
        Err(IndexError::NotEncrypted)
    }
}

fn replace_with_copy(db_path: &Path, copy_path: &Path) -> Result<(), IndexError> {
    for suffix in SIDECAR_SUFFIXES {
        remove_file_if_present(&path_with_suffix(db_path, suffix)?)?;
    }
    std::fs::rename(copy_path, db_path).map_err(IndexError::Io)
}

/// A directory cannot be synced everywhere; the rename has happened either way.
fn sync_parent_directory(db_path: &Path) {
    if let Some(parent) = db_path.parent() {
        let _not_supported_everywhere = File::open(parent).and_then(|dir| dir.sync_all());
    }
}

/// The `x'..'` form skips the passphrase key derivation: the key is already 32
/// random bytes, so stretching it would only cost start-up time.
fn raw_key_text(key: &IndexKey) -> String {
    format!("x'{}'", key.to_hex())
}

fn unlock(conn: &Connection, key: &IndexKey) -> Result<(), IndexError> {
    conn.execute_batch(&format!("PRAGMA key = \"{}\";", raw_key_text(key)))?;
    // Pins the SQLCipher 4 file format. Without it a later SQLCipher major
    // version would apply its own defaults and fail to open existing files.
    conn.execute_batch("PRAGMA cipher_compatibility = 4;")?;
    // Nothing is decrypted until the first read, so a wrong key only shows here.
    match conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| {
        row.get::<_, i64>(0)
    }) {
        Ok(_) => Ok(()),
        Err(rusqlite::Error::SqliteFailure(failure, _))
            if failure.code == ErrorCode::NotADatabase =>
        {
            Err(IndexError::WrongKey)
        }
        Err(err) => Err(err.into()),
    }
}

/// SQLite creates the file on the first write. Writing `user_version` here
/// makes the file exist, in its final (encrypted or plain) form, before any
/// migration has to.
fn write_header_and_migrate(conn: &mut Connection, migrations: &[&str]) -> Result<(), IndexError> {
    conn.pragma_update(None, "user_version", 0)?;
    run_migrations(conn, migrations)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::*;
    use crate::error::KeyStoreError;
    use crate::key_store::InMemoryKeyStore;
    use crate::model::{DocumentId, ReviewReason};
    use crate::test_support::{encrypted_index, new_document, plain_index};

    const TABLE_A: &str = "CREATE TABLE a (id INTEGER);";
    const TABLE_B: &str = "CREATE TABLE b (id INTEGER);";

    fn db_in(dir: &TempDir) -> PathBuf {
        dir.path().join("index.db")
    }

    fn create_encrypted_db(path: &Path, store: &InMemoryKeyStore) -> RecoveryKey {
        let (index, recovery_key) = Index::create_encrypted(path, store).unwrap();
        drop(index);
        recovery_key
    }

    fn table_exists(index: &Index, name: &str) -> bool {
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

    fn user_version(index: &Index) -> u32 {
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
    fn a_version_1_database_keeps_its_documents_and_gains_the_location_columns() {
        let dir = TempDir::new().unwrap();
        let path = db_in(&dir);
        let store = InMemoryKeyStore::empty();
        let (old, _) =
            Index::create_encrypted_with_migrations(&path, &store, &MIGRATIONS[..1]).unwrap();
        assert_eq!(user_version(&old), 1);
        old.conn
            .execute(
                "INSERT INTO document (hash, path, source, received_at, status)
                 VALUES (zeroblob(32), 'inbox/a.pdf', 'inbox', 1, 'new')",
                [],
            )
            .unwrap();
        drop(old);

        let index = Index::open(&path, &store).unwrap();

        assert_eq!(
            user_version(&index),
            u32::try_from(MIGRATIONS.len()).unwrap()
        );
        let document = index
            .document_by_path(std::path::Path::new("inbox/a.pdf"))
            .unwrap()
            .unwrap();
        assert_eq!(document.stamp, None);
        assert!(table_exists(&index, "duplicate_path"));
        index
            .record_duplicate_path(
                document.id,
                std::path::Path::new("inbox/b.pdf"),
                crate::FileStamp {
                    size: 1,
                    mtime: std::time::UNIX_EPOCH,
                },
            )
            .unwrap();
    }

    #[test]
    fn a_version_2_database_gains_the_location_profile_tables() {
        let dir = TempDir::new().unwrap();
        let path = db_in(&dir);
        let store = InMemoryKeyStore::empty();
        let (old, _) =
            Index::create_encrypted_with_migrations(&path, &store, &MIGRATIONS[..2]).unwrap();
        assert_eq!(user_version(&old), 2);
        assert!(!table_exists(&old, "location_profile"));
        drop(old);

        let index = Index::open(&path, &store).unwrap();

        assert_eq!(
            user_version(&index),
            u32::try_from(MIGRATIONS.len()).unwrap()
        );
        assert!(table_exists(&index, "location_profile"));
        assert!(table_exists(&index, "location_subtree"));
    }

    #[test]
    fn a_version_3_database_gains_the_rule_evidence_table() {
        let dir = TempDir::new().unwrap();
        let path = db_in(&dir);
        let store = InMemoryKeyStore::empty();
        let (old, _) =
            Index::create_encrypted_with_migrations(&path, &store, &MIGRATIONS[..3]).unwrap();
        assert_eq!(user_version(&old), 3);
        assert!(!table_exists(&old, "rule_evidence"));
        drop(old);

        let index = Index::open(&path, &store).unwrap();

        assert_eq!(
            user_version(&index),
            u32::try_from(MIGRATIONS.len()).unwrap()
        );
        assert!(table_exists(&index, "rule_evidence"));
    }

    #[test]
    fn a_version_4_database_gains_the_discarded_suggestions_table() {
        let dir = TempDir::new().unwrap();
        let path = db_in(&dir);
        let store = InMemoryKeyStore::empty();
        let (old, _) =
            Index::create_encrypted_with_migrations(&path, &store, &MIGRATIONS[..4]).unwrap();
        assert_eq!(user_version(&old), 4);
        assert!(!table_exists(&old, "discarded_rule_suggestion"));
        drop(old);

        let index = Index::open(&path, &store).unwrap();

        assert_eq!(
            user_version(&index),
            u32::try_from(MIGRATIONS.len()).unwrap()
        );
        assert!(table_exists(&index, "discarded_rule_suggestion"));
    }

    #[test]
    fn a_version_5_database_gains_the_reorganisation_plan_tables() {
        let dir = TempDir::new().unwrap();
        let path = db_in(&dir);
        let store = InMemoryKeyStore::empty();
        let (old, _) =
            Index::create_encrypted_with_migrations(&path, &store, &MIGRATIONS[..5]).unwrap();
        assert_eq!(user_version(&old), 5);
        assert!(!table_exists(&old, "reorganisation_plan"));
        drop(old);

        let index = Index::open(&path, &store).unwrap();

        assert_eq!(
            user_version(&index),
            u32::try_from(MIGRATIONS.len()).unwrap()
        );
        for table in [
            "reorganisation_plan",
            "reorganisation_item",
            "reorganisation_finding",
        ] {
            assert!(table_exists(&index, table), "{table}");
        }
    }

    #[test]
    fn a_version_6_database_gains_the_execution_journal_tables() {
        let dir = TempDir::new().unwrap();
        let path = db_in(&dir);
        let store = InMemoryKeyStore::empty();
        let (old, _) =
            Index::create_encrypted_with_migrations(&path, &store, &MIGRATIONS[..6]).unwrap();
        assert_eq!(user_version(&old), 6);
        assert!(!table_exists(&old, "execution_batch"));
        drop(old);

        let index = Index::open(&path, &store).unwrap();

        assert_eq!(
            user_version(&index),
            u32::try_from(MIGRATIONS.len()).unwrap()
        );
        for table in ["execution_batch", "execution_item", "execution_created_dir"] {
            assert!(table_exists(&index, table), "{table}");
        }
    }

    #[test]
    fn a_version_10_database_turns_its_rule_columns_into_conditions() {
        use crate::model::{
            Combinator, Condition, ConditionKind, DocumentId, FieldOrigin, RuleConditions, RuleId,
        };

        let dir = TempDir::new().unwrap();
        let path = db_in(&dir);
        let old = Index::create_plain_with_migrations(&path, &MIGRATIONS[..10]).unwrap();
        assert_eq!(user_version(&old), 10);
        old.conn
            .execute_batch(
                "INSERT INTO document_kind (id, name, name_key) VALUES (1, 'Invoice', 'invoice');
                 INSERT INTO sender (id, name, name_key) VALUES (1, 'Company A', 'company a');
                 INSERT INTO tag (id, name, name_key) VALUES (1, 'services', 'services');
                 INSERT INTO rule (id, match_text, match_text_is_regex, match_sender_id,
                     kind_id, tax_context, filing_pattern, confirmed)
                 VALUES (3, 'Invoice', 0, NULL, 1, 'company', '{year}', 1),
                        (5, 'Invoice \\d+', 1, NULL, 1, 'private', '{year}', 0),
                        (8, NULL, 0, 1, 1, 'none', '{sender}', 1);
                 INSERT INTO rule_tag (rule_id, tag_id) VALUES (8, 1);
                 INSERT INTO rule_evidence
                     (rule_id, matching_in_folder, folder_documents, matching_elsewhere, updated_at)
                 VALUES (8, 4, 5, 1, 1700000000);
                 INSERT INTO document (id, hash, path, source, received_at, status)
                 VALUES (1, zeroblob(32), 'inbox/a.pdf', 'inbox', 1700000000, 'planned');
                 INSERT INTO document_field_origin (document_id, field, origin, rule_id)
                 VALUES (1, 'kind', 'rule', 8);",
            )
            .unwrap();
        drop(old);

        let index = Index::open(&path, &InMemoryKeyStore::empty()).unwrap();

        assert_eq!(user_version(&index), latest_version());
        let rules = index.rules().unwrap();
        let ids: Vec<_> = rules.iter().map(|rule| rule.id).collect();
        assert_eq!(ids, [RuleId::new(3), RuleId::new(5), RuleId::new(8)]);
        let conditions: Vec<_> = rules.iter().map(|rule| &rule.spec.conditions).collect();
        assert_eq!(
            conditions,
            [
                &RuleConditions::all_of(vec![Condition::new(ConditionKind::Substring, "Invoice")]),
                &RuleConditions::all_of(vec![Condition::new(ConditionKind::Regex, "Invoice \\d+")]),
                &RuleConditions::all_of(vec![Condition::new(ConditionKind::Sender, "Company A")]),
            ]
        );
        assert!(rules
            .iter()
            .all(|rule| rule.spec.conditions.combinator == Combinator::AllOf));
        assert!(rules[0].spec.confirmed && !rules[1].spec.confirmed);
        let sender_rule = index.rule(RuleId::new(8)).unwrap().unwrap();
        assert_eq!(sender_rule.spec.tags.len(), 1);
        assert!(sender_rule.spec.tags.contains("services"));
        assert!(index.rule_evidence(RuleId::new(8)).unwrap().is_some());
        let origins = index.field_origins(DocumentId::new(1)).unwrap();
        assert_eq!(
            origins.of(crate::model::Field::Kind),
            FieldOrigin::Rule(Some(RuleId::new(8)))
        );
        let violations = index
            .conn
            .prepare("PRAGMA foreign_key_check")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .count();
        assert_eq!(violations, 0);
    }

    #[test]
    fn a_version_11_database_gains_sender_aliases() {
        use crate::model::{
            Condition, ConditionKind, DocumentId, RuleConditions, RuleId, SenderAliasKind,
        };

        let dir = TempDir::new().unwrap();
        let path = db_in(&dir);
        let old = Index::create_plain_with_migrations(&path, &MIGRATIONS[..11]).unwrap();
        assert_eq!(user_version(&old), 11);
        old.conn
            .execute_batch(
                "INSERT INTO sender (id, name, name_key) VALUES (1, 'Company A', 'company a');
                 INSERT INTO document (id, hash, path, source, received_at, status, sender_id)
                 VALUES (1, zeroblob(32), 'inbox/a.pdf', 'inbox', 1700000000, 'planned', 1);
                 INSERT INTO document_kind (id, name, name_key) VALUES (1, 'Invoice', 'invoice');
                 INSERT INTO rule (id, kind_id, tax_context, filing_pattern, confirmed)
                 VALUES (4, 1, 'none', '{year}', 1);
                 INSERT INTO rule_condition (rule_id, position, kind, value)
                 VALUES (4, 0, 'sender', 'Company A');",
            )
            .unwrap();
        drop(old);

        let index = Index::open(&path, &InMemoryKeyStore::empty()).unwrap();

        assert_eq!(user_version(&index), latest_version());
        let document = index.document(DocumentId::new(1)).unwrap().unwrap();
        assert_eq!(document.fields.sender.as_deref(), Some("Company A"));
        let rule = index.rule(RuleId::new(4)).unwrap().unwrap();
        assert_eq!(
            rule.spec.conditions,
            RuleConditions::all_of(vec![Condition::new(ConditionKind::Sender, "Company A")])
        );
        assert!(index.sender_aliases().unwrap().is_empty());
        assert!(index
            .offer_sender_alias("Company A", SenderAliasKind::Domain, "company-a.example")
            .unwrap());
        let violations = index
            .conn
            .prepare("PRAGMA foreign_key_check")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .count();
        assert_eq!(violations, 0);
    }

    #[test]
    fn a_version_12_database_keeps_its_rule_order_and_enables_every_rule() {
        use crate::model::{DocumentId, Field, FieldOrigin, RuleId};

        let dir = TempDir::new().unwrap();
        let path = db_in(&dir);
        let old = Index::create_plain_with_migrations(&path, &MIGRATIONS[..12]).unwrap();
        assert_eq!(user_version(&old), 12);
        old.conn
            .execute_batch(
                "INSERT INTO document_kind (id, name, name_key) VALUES (1, 'Invoice', 'invoice');
                 INSERT INTO rule (id, kind_id, tax_context, filing_pattern, confirmed)
                 VALUES (7, 1, 'none', '{year}', 1),
                        (2, 1, 'none', '{year}', 1),
                        (5, 1, 'none', '{year}', 0);
                 INSERT INTO rule_condition (rule_id, position, kind, value)
                 VALUES (7, 0, 'substring', 'a'), (2, 0, 'substring', 'b'),
                        (5, 0, 'substring', 'c');
                 INSERT INTO document (id, hash, path, source, received_at, status)
                 VALUES (1, zeroblob(32), 'inbox/a.pdf', 'inbox', 1700000000, 'planned');
                 INSERT INTO document_field_origin (document_id, field, origin, rule_id)
                 VALUES (1, 'kind', 'rule', 5);",
            )
            .unwrap();
        drop(old);

        let index = Index::open(&path, &InMemoryKeyStore::empty()).unwrap();

        assert_eq!(user_version(&index), latest_version());
        let rules = index.rules().unwrap();
        let ids: Vec<_> = rules.iter().map(|rule| rule.id).collect();
        assert_eq!(ids, [RuleId::new(2), RuleId::new(5), RuleId::new(7)]);
        let positions: Vec<_> = rules.iter().map(|rule| rule.position).collect();
        assert_eq!(positions, [2, 5, 7]);
        assert!(rules.iter().all(|rule| rule.spec.enabled));
        let origins = index.field_origins(DocumentId::new(1)).unwrap();
        assert_eq!(
            origins.of(Field::Kind),
            FieldOrigin::Rule(Some(RuleId::new(5)))
        );
        let violations = index
            .conn
            .prepare("PRAGMA foreign_key_check")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .count();
        assert_eq!(violations, 0);
    }

    #[test]
    fn a_version_13_database_keeps_its_discarded_suggestions_as_word_suggestions() {
        use crate::model::{
            Condition, ConditionKind, RuleConditions, RuleId, RuleSpec, RuleSuggestion,
        };

        let dir = TempDir::new().unwrap();
        let path = db_in(&dir);
        let old = Index::create_plain_with_migrations(&path, &MIGRATIONS[..13]).unwrap();
        assert_eq!(user_version(&old), 13);
        old.conn
            .execute_batch(
                "INSERT INTO discarded_rule_suggestion
                     (match_text, kind, filing_pattern, discarded_at)
                 VALUES ('company a ltd', 'insurance', '{year}', 1700000000);
                 INSERT INTO document_kind (id, name, name_key) VALUES (1, 'Insurance', 'insurance');
                 INSERT INTO rule (id, kind_id, tax_context, filing_pattern, confirmed)
                 VALUES (3, 1, 'none', 'Insurance/{date}', 0);
                 INSERT INTO rule_condition (rule_id, position, kind, value)
                 VALUES (3, 0, 'word', 'agency b');
                 INSERT INTO rule_evidence
                     (rule_id, matching_in_folder, folder_documents, matching_elsewhere, updated_at)
                 VALUES (3, 5, 5, 0, 1700000000);",
            )
            .unwrap();
        drop(old);

        let mut index = Index::open(&path, &InMemoryKeyStore::empty()).unwrap();

        assert_eq!(user_version(&index), latest_version());
        let evidence = index.rule_evidence(RuleId::new(3)).unwrap().unwrap();
        assert_eq!(evidence.matching_in_folder, 5);
        assert_eq!(evidence.subtree, None);
        let suggestion = |kind| RuleSpec {
            name: None,
            conditions: RuleConditions::all_of(vec![Condition::new(kind, "Company A Ltd")]),
            extract_number: None,
            extract_date: None,
            extract_amount: None,
            kind: Some("Insurance".to_owned()),
            tags: Default::default(),
            filing_pattern: "{year}/".to_owned(),
            confirmed: false,
            enabled: true,
        };
        assert_eq!(
            index
                .suggest_rule(&suggestion(ConditionKind::Word), evidence.clone())
                .unwrap(),
            RuleSuggestion::Discarded
        );
        assert!(matches!(
            index
                .suggest_rule(&suggestion(ConditionKind::Sender), evidence)
                .unwrap(),
            RuleSuggestion::Created(_)
        ));
    }

    #[test]
    fn a_version_14_database_keeps_its_sender_aliases_and_offers_a_discarded_value_to_another_sender(
    ) {
        use crate::model::{SenderAliasKind, SenderAliasState};

        let dir = TempDir::new().unwrap();
        let path = db_in(&dir);
        let old = Index::create_plain_with_migrations(&path, &MIGRATIONS[..14]).unwrap();
        assert_eq!(user_version(&old), 14);
        old.conn
            .execute_batch(
                "INSERT INTO sender (id, name, name_key) VALUES (1, 'A GmbH', 'a gmbh');
                 INSERT INTO sender (id, name, name_key) VALUES (2, 'B GmbH', 'b gmbh');
                 INSERT INTO sender_alias (id, sender_id, kind, value, state)
                 VALUES (7, 1, 'domain', 'a.example', 'offered');
                 INSERT INTO sender_alias (id, sender_id, kind, value, state)
                 VALUES (9, 1, 'domain', 'shared.example', 'discarded');",
            )
            .unwrap();
        drop(old);

        let index = Index::open(&path, &InMemoryKeyStore::empty()).unwrap();

        assert_eq!(user_version(&index), latest_version());
        let aliases = index.sender_aliases().unwrap();
        let kept: Vec<_> = aliases
            .iter()
            .map(|alias| (alias.id.get(), alias.sender.as_str(), alias.state))
            .collect();
        assert_eq!(
            kept,
            [
                (7, "A GmbH", SenderAliasState::Offered),
                (9, "A GmbH", SenderAliasState::Discarded)
            ]
        );
        assert!(!index
            .offer_sender_alias("A GmbH", SenderAliasKind::Domain, "shared.example")
            .unwrap());
        assert!(index
            .offer_sender_alias("B GmbH", SenderAliasKind::Domain, "shared.example")
            .unwrap());
        assert!(!index
            .offer_sender_alias("B GmbH", SenderAliasKind::Domain, "a.example")
            .unwrap());
    }

    #[test]
    fn a_version_17_database_gains_the_origin_of_its_rules() {
        use crate::model::{RuleId, RuleOrigin};

        let dir = TempDir::new().unwrap();
        let path = db_in(&dir);
        let old = Index::create_plain_with_migrations(&path, &MIGRATIONS[..17]).unwrap();
        assert_eq!(user_version(&old), 17);
        old.conn
            .execute_batch(
                "INSERT INTO document_kind (id, name, name_key) VALUES (1, 'Invoice', 'invoice');
                 INSERT INTO rule (id, kind_id, tax_context_id, filing_pattern, confirmed)
                 VALUES (1, 1, 0, '{year}', 0), (2, 1, 0, '{year}', 1);
                 INSERT INTO rule_condition (rule_id, position, kind, value)
                 VALUES (1, 0, 'word', 'rent'), (2, 0, 'word', 'tax');
                 INSERT INTO rule_evidence
                     (rule_id, matching_in_folder, folder_documents, matching_elsewhere, updated_at)
                 VALUES (1, 3, 3, 0, 1700000000);",
            )
            .unwrap();
        drop(old);

        let index = Index::open(&path, &InMemoryKeyStore::empty()).unwrap();

        assert_eq!(user_version(&index), latest_version());
        let origin_of = |id: i64| index.rule(RuleId::new(id)).unwrap().unwrap().origin;
        assert_eq!(origin_of(1), RuleOrigin::Suggestion);
        assert_eq!(origin_of(2), RuleOrigin::Filing);
    }

    /// A version 19 database with the data a tax context touched, opened so
    /// that migration 020 runs.
    fn migrated_from_19() -> (TempDir, Index) {
        let dir = TempDir::new().unwrap();
        let path = db_in(&dir);
        let old = Index::create_plain_with_migrations(&path, &MIGRATIONS[..19]).unwrap();
        assert_eq!(user_version(&old), 19);
        old.conn
            .execute_batch(
                "INSERT INTO tax_context (id, name, short_name, combinator)
                     VALUES (3, 'Firma A', 'firma-a', 'any'), (4, 'Firma B', 'firma-b', 'any');
                 INSERT INTO tax_context_condition (tax_context_id, position, kind, value)
                     VALUES (3, 0, 'recipient', 'Firma A GmbH'), (4, 0, 'recipient', 'Firma B');
                 INSERT INTO sender (id, name, name_key) VALUES (1, 'Company A', 'company a');
                 INSERT INTO document_kind (id, name, name_key) VALUES (1, 'Invoice', 'invoice');
                 INSERT INTO tag (id, name, name_key) VALUES (1, 'services', 'services');
                 INSERT INTO document (id, hash, path, source, received_at, status, sender_id,
                         kind_id, tax_context_id, document_date, document_number, amount_minor,
                         currency, file_size, file_mtime)
                     VALUES (1, zeroblob(32), 'a.pdf', 'inbox', 1700000000, 'in_review', 1, 1, 3,
                         '2026-02-28', '4711', 11900, 'EUR', 10, 20),
                         (2, randomblob(32), 'b.pdf', 'inbox', 1700000000, 'in_review', NULL, NULL,
                         NULL, NULL, NULL, NULL, NULL, NULL, NULL),
                         (3, randomblob(32), 'c.pdf', 'inbox', 1700000000, 'planned', NULL, NULL,
                         4, NULL, NULL, NULL, NULL, NULL, NULL);
                 INSERT INTO document_text (rowid, body) VALUES (1, 'Rechnung fuer die Miete');
                 INSERT INTO document_tag (document_id, tag_id) VALUES (1, 1);
                 INSERT INTO rule (id, kind_id, tax_context_id, filing_pattern, confirmed)
                     VALUES (3, 1, 3, '{tax_context}/{year}/{sender}', 1),
                            (4, 1, 4, '{tax_context}', 0),
                            (5, 1, 0, '{year}/{number}', 1),
                            (6, 1, 0, '{year}/{year}{month}{day}_{sender}', 1);
                 INSERT INTO rule_condition (rule_id, position, kind, value)
                     VALUES (3, 0, 'word', 'rent'), (4, 0, 'word', 'tax'),
                            (5, 0, 'word', 'a'), (6, 0, 'word', 'b');
                 INSERT INTO document_field_origin
                         (document_id, field, origin, rule_id, confidence, condition_kind,
                          condition_value)
                     VALUES (1, 'sender', 'rule', 3, NULL, NULL, NULL),
                            (1, 'tax_context', 'condition', NULL, NULL, 'recipient', 'Firma A GmbH');
                 INSERT INTO review_entry (id, document_id, reason, field, created_at, resolved_at)
                     VALUES (1, 1, 'missing_field', 'tax_context', 1700000000, NULL),
                            (2, 2, 'no_tax_context_matches', NULL, 1700000000, NULL),
                            (3, 3, 'several_tax_contexts_match', NULL, 1700000000, 1700000100),
                            (4, 2, 'rule_conflict', NULL, 1700000000, 1700000200);
                 INSERT INTO discarded_rule_suggestion
                         (match_text, condition_kind, kind, filing_pattern, discarded_at)
                     VALUES ('company a ltd', 'word', 'insurance', '{tax_context}/{year}', 1),
                            ('company a ltd', 'word', 'insurance', '{year}', 2);
                 INSERT INTO location_profile (location, learned_at) VALUES ('archive', 1);
                 INSERT INTO location_subtree (location, subtree, pattern, documents, total)
                     VALUES ('archive', 'a', '{year}/{tax_context}', 3, 3),
                            ('archive', 'b', NULL, 1, 1),
                            ('archive', 'c', '{year}/{date}_{sender}', 2, 2);
                 INSERT INTO saved_query (id, name, year, tax_context_id, kind, created_at, sender)
                     VALUES (7, 'Firma A only', 2026, 3, 'Invoice', 1700000000, 'Company A');",
            )
            .unwrap();
        drop(old);

        let index = Index::open(&path, &InMemoryKeyStore::empty()).unwrap();
        (dir, index)
    }

    #[test]
    fn a_version_19_database_loses_its_tax_context_tables_and_columns() {
        use crate::model::RuleId;

        let (_dir, index) = migrated_from_19();

        assert_eq!(user_version(&index), latest_version());
        assert_eq!(user_version(&index), 20);
        let violations = index
            .conn
            .prepare("PRAGMA foreign_key_check")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .count();
        assert_eq!(violations, 0);
        assert!(!table_exists(&index, "tax_context"));
        assert!(!table_exists(&index, "tax_context_condition"));
        for (table, column) in [
            ("document", "tax_context_id"),
            ("rule", "tax_context_id"),
            ("saved_query", "tax_context_id"),
            ("document_field_origin", "condition_kind"),
        ] {
            assert!(!has_column(&index, table, column), "{table}.{column}");
        }
        assert!(has_column(&index, "document", "recipient"));
        assert!(has_column(&index, "rule", "name"));

        let rules = index.rules().unwrap();
        assert_eq!(
            rules.iter().map(|rule| rule.id).collect::<Vec<_>>(),
            [
                RuleId::new(3),
                RuleId::new(4),
                RuleId::new(5),
                RuleId::new(6)
            ]
        );
        assert_eq!(rules[0].spec.filing_pattern, "{year}/{sender}/");
        assert_eq!(rules[1].spec.filing_pattern, "{year}/");
        assert_eq!(rules[2].spec.filing_pattern, "{year}/{number}");
        assert_eq!(
            rules[3].spec.filing_pattern,
            "{year}/{year}{month}{day}_{sender}"
        );
        assert_eq!(rules[0].spec.conditions.list.len(), 1);
        index
            .conn
            .execute("UPDATE rule SET kind_id = NULL WHERE id = 3", [])
            .unwrap();
        let open_kind: Option<i64> = index
            .conn
            .query_row("SELECT kind_id FROM rule WHERE id = 3", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(open_kind, None);
    }

    #[test]
    fn a_version_19_database_keeps_its_discarded_suggestions_and_turns_reviews_into_no_rule() {
        use crate::model::{DocumentId, Field, FieldOrigin, ReviewReason, RuleId};

        let (_dir, index) = migrated_from_19();

        let kept: i64 = index
            .conn
            .query_row(
                "SELECT count(*) FROM discarded_rule_suggestion",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(kept, 1);
        let subtree_patterns: Vec<Option<String>> = index
            .conn
            .prepare("SELECT pattern FROM location_subtree ORDER BY subtree")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            subtree_patterns,
            [
                Some("{year}/".to_owned()),
                None,
                Some("{year}/{date}_{sender}".to_owned())
            ]
        );

        let reasons: Vec<ReviewReason> = index
            .conn
            .prepare("SELECT reason, field FROM review_entry ORDER BY id")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .unwrap()
            .map(|row| {
                let (reason, field) = row.unwrap();
                ReviewReason::parse(&reason, field.as_deref()).unwrap()
            })
            .collect();
        assert_eq!(reasons, [ReviewReason::NoMatchingRule; 4]);
        let open: Vec<_> = index
            .open_reviews()
            .unwrap()
            .into_iter()
            .map(|entry| (entry.document_id, entry.reason))
            .collect();
        assert_eq!(
            open,
            [
                (DocumentId::new(1), ReviewReason::NoMatchingRule),
                (DocumentId::new(2), ReviewReason::NoMatchingRule)
            ]
        );

        let origins = index.field_origins(DocumentId::new(1)).unwrap();
        assert_eq!(
            origins.of(Field::Sender),
            FieldOrigin::Rule(Some(RuleId::new(3)))
        );
        assert_eq!(origins.of(Field::Kind), FieldOrigin::Unknown);
        let origin_rows: i64 = index
            .conn
            .query_row("SELECT count(*) FROM document_field_origin", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(origin_rows, 1);
    }

    #[test]
    fn a_version_19_database_keeps_the_fields_tags_and_reports_of_its_documents() {
        use crate::model::DocumentId;

        let (_dir, index) = migrated_from_19();

        let document = index.document(DocumentId::new(1)).unwrap().unwrap();
        assert_eq!(document.fields.sender.as_deref(), Some("Company A"));
        assert_eq!(document.fields.number.as_deref(), Some("4711"));
        assert!(document.fields.tags.contains("services"));
        let automatic: i64 = index
            .conn
            .query_row("SELECT sum(automatic) FROM document_tag", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(automatic, 0);
        let queries = index.saved_queries().unwrap();
        assert_eq!(queries.len(), 1);
        assert_eq!(queries[0].spec.year, Some(2026));
        assert_eq!(queries[0].spec.sender.as_deref(), Some("Company A"));
    }

    #[test]
    fn a_version_19_database_keeps_the_text_search_and_the_cascade_of_its_documents() {
        use crate::model::DocumentId;

        let (_dir, index) = migrated_from_19();

        assert_eq!(
            index.search_text("miete", 10).unwrap(),
            [DocumentId::new(1)]
        );
        let by_path: i64 = index
            .conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name = 'document_by_path'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(by_path, 1);
        index
            .conn
            .execute("DELETE FROM document WHERE id = 1", [])
            .unwrap();
        let text_left: i64 = index
            .conn
            .query_row("SELECT count(*) FROM document_text", [], |row| row.get(0))
            .unwrap();
        assert_eq!(text_left, 0);
    }

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

    #[test]
    fn statistics_of_a_fresh_index_are_empty_in_both_flavours() {
        for (_dir, index) in [encrypted_index(), plain_index()] {
            let statistics = index.statistics().unwrap();

            assert_eq!(
                usize::try_from(statistics.schema_version).unwrap(),
                MIGRATIONS.len()
            );
            assert_eq!(statistics.document_count, 0);
            assert_eq!(statistics.open_reviews, 0);
            assert!(statistics.full_text_bytes > 0);
        }
    }

    #[test]
    fn statistics_follow_documents_text_and_reviews() {
        let (_dir, mut index) = encrypted_index();
        let before = index.statistics().unwrap().full_text_bytes;
        let document = index.insert_document(&new_document(1)).unwrap();
        index
            .set_document_text(document, &"Invoice from Company A. ".repeat(4000))
            .unwrap();

        let filled = index.statistics().unwrap();
        assert_eq!(filled.document_count, 1);
        assert!(filled.full_text_bytes > before);

        let entry = index
            .add_review(document, ReviewReason::NoMatchingRule)
            .unwrap();
        assert_eq!(index.statistics().unwrap().open_reviews, 1);
        index.resolve_review(entry).unwrap();
        assert_eq!(index.statistics().unwrap().open_reviews, 0);
    }

    const FIXTURE_V0_6_0: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/index-v0.6.0.db"
    );
    const PROBE_COLUMN: &str = "ALTER TABLE document ADD COLUMN probe INTEGER";

    /// A copy of the index release 0.6.0 wrote, and the store holding the key
    /// it was written with (bytes 0x00 to 0x1f).
    fn copy_of_the_v0_6_0_index() -> (TempDir, PathBuf, InMemoryKeyStore) {
        let dir = TempDir::new().unwrap();
        let path = db_in(&dir);
        fs::copy(FIXTURE_V0_6_0, &path).unwrap();
        let key: Vec<u8> = (0..32).collect();
        (dir, path, InMemoryKeyStore::with_entry(key))
    }

    const FIXTURE_V0_6_0_VERSION: u32 = 9;

    fn latest_version() -> u32 {
        u32::try_from(MIGRATIONS.len()).unwrap()
    }

    fn probe_version() -> u32 {
        latest_version() + 1
    }

    fn migrations_and(extra: &[&'static str]) -> Vec<&'static str> {
        MIGRATIONS.iter().chain(extra).copied().collect()
    }

    fn has_column(index: &Index, table: &str, column: &str) -> bool {
        index
            .conn
            .query_row(
                &format!("SELECT count(*) FROM pragma_table_info('{table}') WHERE name = ?1"),
                [column],
                |row| row.get::<_, i64>(0),
            )
            .unwrap()
            == 1
    }

    fn assert_v0_6_0_content(index: &Index) {
        use crate::model::{DocumentSource, DocumentStatus};

        assert!(index.is_encrypted());
        let invoice = index
            .document_by_path(Path::new("archive/2026/Company A/invoice-4711.pdf"))
            .unwrap()
            .unwrap();
        assert_eq!(invoice.id.get(), 1);
        assert_eq!(invoice.source, DocumentSource::Archive);
        assert_eq!(invoice.status, DocumentStatus::Filed);
        assert_eq!(invoice.fields.sender.as_deref(), Some("Company A"));
        assert_eq!(invoice.fields.kind.as_deref(), Some("Invoice"));
        assert_eq!(invoice.fields.number.as_deref(), Some("4711"));
        assert_eq!(
            invoice.fields.date.map(|date| date.to_string()).as_deref(),
            Some("2026-01-15")
        );
        let amount = invoice.fields.amount.clone().unwrap();
        assert_eq!(
            (amount.minor_units, amount.currency.as_str()),
            (11900, "EUR")
        );
        assert_eq!(
            invoice
                .fields
                .tags
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["2026", "services"]
        );

        let receipt = index
            .document_by_path(Path::new("inbox/scan-2.pdf"))
            .unwrap()
            .unwrap();
        assert_eq!(
            (receipt.id.get(), receipt.status),
            (2, DocumentStatus::Planned)
        );
        let notice = index
            .document_by_path(Path::new("inbox/scan-3.pdf"))
            .unwrap()
            .unwrap();
        assert_eq!(
            (notice.id.get(), notice.status),
            (3, DocumentStatus::InReview)
        );

        assert_v0_6_0_search_and_other_tables(index, &invoice, &receipt, &notice);
    }

    fn assert_v0_6_0_search_and_other_tables(
        index: &Index,
        invoice: &crate::model::Document,
        receipt: &crate::model::Document,
        notice: &crate::model::Document,
    ) {
        use crate::model::{Condition, ConditionKind, ReviewReason, RuleConditions};

        assert_eq!(index.search_text("hosting", 10).unwrap(), [invoice.id]);
        assert_eq!(index.search_text("chair", 10).unwrap(), [receipt.id]);
        assert_eq!(index.search_text("Finanzamt", 10).unwrap(), [notice.id]);
        assert!(index.search_text("nonexistentword", 10).unwrap().is_empty());

        let rules = index.rules().unwrap();
        assert_eq!(rules.len(), 1);
        let rule = &rules[0].spec;
        assert_eq!(
            rule.conditions,
            RuleConditions::all_of(vec![
                Condition::new(ConditionKind::Substring, "Invoice"),
                Condition::new(ConditionKind::Sender, "Company A"),
            ])
        );
        assert_eq!(rule.filing_pattern, "{year}/{sender}/");
        assert!(rule.confirmed);

        let reviews = index.open_reviews().unwrap();
        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0].document_id, notice.id);
        assert_eq!(reviews[0].reason, ReviewReason::NoMatchingRule);

        let profile = index.location_profile(Path::new("archive")).unwrap();
        assert_eq!(profile.len(), 1);
        assert_eq!(profile[0].subtree, "2026");
        assert_eq!(profile[0].pattern.as_deref(), Some("{year}/{sender}/"));

        let queries = index.saved_queries().unwrap();
        assert_eq!(queries.len(), 1);
        assert_eq!(queries[0].name, "Invoices 2026");
        assert_eq!(queries[0].spec.year, Some(2026));
        assert_eq!(queries[0].spec.full_text.as_deref(), Some("hosting"));
    }

    #[test]
    fn an_index_written_by_release_0_6_0_opens_with_all_its_content() {
        let (_dir, path, store) = copy_of_the_v0_6_0_index();

        let index = Index::open_with_migrations(&path, &store, MIGRATIONS).unwrap();

        assert_eq!(
            user_version(&index),
            u32::try_from(MIGRATIONS.len()).unwrap()
        );
        assert_v0_6_0_content(&index);
        let invoice = index
            .document_by_path(Path::new("archive/2026/Company A/invoice-4711.pdf"))
            .unwrap()
            .unwrap();
        let origins = index.field_origins(invoice.id).unwrap();
        for field in crate::model::Field::ALL {
            assert_eq!(origins.of(field), crate::model::FieldOrigin::Unknown);
        }
    }

    #[test]
    fn a_later_step_migrates_the_release_0_6_0_index_and_keeps_its_content() {
        let (_dir, path, store) = copy_of_the_v0_6_0_index();
        let steps = migrations_and(&[PROBE_COLUMN]);

        let index = Index::open_with_migrations(&path, &store, &steps).unwrap();

        assert_eq!(user_version(&index), probe_version());
        assert!(has_column(&index, "document", "probe"));
        assert_v0_6_0_content(&index);
        drop(index);
        let reopened = Index::open_with_migrations(&path, &store, &steps).unwrap();
        assert_eq!(user_version(&reopened), probe_version());
    }

    #[test]
    fn a_failing_second_step_leaves_the_release_0_6_0_file_byte_for_byte_as_it_was() {
        let (_dir, path, store) = copy_of_the_v0_6_0_index();
        let before = fs::read(&path).unwrap();
        let steps = migrations_and(&[PROBE_COLUMN, "CREATE TABLE document (x)"]);

        let err = Index::open_with_migrations(&path, &store, &steps).unwrap_err();

        assert!(
            matches!(
                err,
                IndexError::MigrationFailed { from, step, .. }
                    if from == FIXTURE_V0_6_0_VERSION && step == latest_version() + 2
            ),
            "{err}"
        );
        assert!(err.to_string().contains("left unchanged"), "{err}");
        assert_eq!(fs::read(&path).unwrap(), before);
        let index = Index::open_with_migrations(&path, &store, MIGRATIONS).unwrap();
        assert_eq!(user_version(&index), latest_version());
        assert!(!has_column(&index, "document", "probe"));
        assert_v0_6_0_content(&index);
    }

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
}
