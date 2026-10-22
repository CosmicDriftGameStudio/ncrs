use std::fs::File;
use std::io::Read as _;
use std::path::Path;

use rusqlite::{Connection, ErrorCode};

use crate::error::IndexError;
use crate::key::{IndexKey, RecoveryKey};
use crate::key_store::KeyStore;
use crate::migrations::{run_migrations, MIGRATIONS};

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

    pub fn is_encrypted(&self) -> bool {
        self.encrypted
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
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

fn unlock(conn: &Connection, key: &IndexKey) -> Result<(), IndexError> {
    // The raw `x'..'` form skips the passphrase key derivation: the key is
    // already 32 random bytes, so stretching it would only cost start-up time.
    conn.execute_batch(&format!("PRAGMA key = \"x'{}'\";", key.to_hex()))?;
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
        assert!(matches!(err, IndexError::Sqlite(_)), "{err}");

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

        assert_eq!(user_version(&index), 2);
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
        assert!(matches!(err, IndexError::ForeignKeyViolation), "{err}");

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
}
