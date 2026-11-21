use std::fs::File;
use std::io::{ErrorKind, Read as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use rusqlite::{Connection, ErrorCode};
use zeroize::Zeroizing;

use crate::error::IndexError;
use crate::folder_pick::create_folder_pick_table;
use crate::key::{IndexKey, RecoveryKey};
use crate::key_store::KeyStore;
use crate::kinds::KindCatalog;
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

static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(1);

fn next_instance() -> u64 {
    NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed)
}

/// Differs whenever the index was written to or replaced by another one; two
/// equal stamps mean nothing was written in between.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChangeStamp {
    instance: u64,
    changed_rows: u64,
}

#[derive(Debug)]
pub struct Index {
    conn: Connection,
    instance: u64,
    encrypted: bool,
    kinds: Arc<KindCatalog>,
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

    /// A consistent copy of an unencrypted index as a new plain file, made
    /// while the index is in use. The target must not exist.
    pub fn save_plain_copy(&self, target: &Path) -> Result<(), IndexError> {
        let text = target
            .to_str()
            .ok_or_else(|| IndexError::NonUtf8Path(target.to_path_buf()))?;
        self.connection().execute("VACUUM INTO ?1", [text])?;
        Ok(())
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
        verify_stored(store, key)
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
        let (key, entry_is_new) = match store.load_protected()? {
            Some(stored) => (IndexKey::from_stored(&stored)?, false),
            None => (IndexKey::generate()?, true),
        };
        let swapped = export_check_and_swap(db_path, &copy_path, &key, store, entry_is_new);
        if swapped.is_err() {
            let _copy_may_not_exist = std::fs::remove_file(&copy_path);
        }
        swapped.map(|()| key.recovery_key())
    }

    /// The recovery key of the key in the store, to show it again when the user
    /// has not yet confirmed it. `None` when the store holds no key.
    pub fn stored_recovery_key(store: &dyn KeyStore) -> Result<Option<RecoveryKey>, IndexError> {
        store
            .load_protected()?
            .map(|stored| IndexKey::from_stored(&stored).map(|key| key.recovery_key()))
            .transpose()
    }

    pub fn is_encrypted(&self) -> bool {
        self.encrypted
    }

    /// What SQLite's quick check finds wrong with the file; none when it is
    /// sound. Only reports: the file is never changed for it.
    pub fn damage(&self) -> Result<Option<String>, IndexError> {
        let mut statement = self.conn.prepare("PRAGMA quick_check")?;
        let lines = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok((lines != ["ok"]).then(|| lines.join("; ")))
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
        let conn = Connection::open(db_path)
            .map_err(IndexError::from)
            .and_then(|conn| {
                unlock(&conn, &key)?;
                Ok(conn)
            })
            .and_then(|mut conn| {
                write_header_and_migrate(&mut conn, migrations)?;
                Ok(conn)
            })
            .map_err(|err| discard_half_created(db_path, err))?;
        let recovery_key = key.recovery_key();
        Ok((
            Self {
                conn,
                instance: next_instance(),
                encrypted: true,
                kinds: Arc::default(),
            },
            recovery_key,
        ))
    }

    fn create_plain_with_migrations(
        db_path: &Path,
        migrations: &[&str],
    ) -> Result<Self, IndexError> {
        require_no_file(db_path)?;
        let conn = Connection::open(db_path)
            .map_err(IndexError::from)
            .and_then(|mut conn| {
                overwrite_deleted_pages(&conn)?;
                write_header_and_migrate(&mut conn, migrations)?;
                Ok(conn)
            })
            .map_err(|err| discard_half_created(db_path, err))?;
        Ok(Self {
            conn,
            instance: next_instance(),
            encrypted: false,
            kinds: Arc::default(),
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
            let stored = store.load_protected()?.ok_or(IndexError::KeyMissing)?;
            unlock(&conn, &IndexKey::from_stored(&stored)?)?;
        }
        if !encrypted {
            overwrite_deleted_pages(&conn)?;
        }
        let applied: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if applied > 0
            && usize::try_from(applied).is_ok_and(|applied_count| applied_count < migrations.len())
        {
            // A copy of the main file alone misses what still waits in a
            // write-ahead log.
            conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
            crate::backup::before_migration(db_path, applied)?;
        }
        run_migrations(&mut conn, migrations)?;
        crate::names::fold_name_keys(&mut conn)?;
        create_folder_pick_table(&conn)?;
        Ok(Self {
            conn,
            instance: next_instance(),
            encrypted,
            kinds: Arc::default(),
        })
    }

    pub fn change_stamp(&self) -> ChangeStamp {
        ChangeStamp {
            instance: self.instance,
            changed_rows: self.conn.total_changes(),
        }
    }

    /// The kinds this index knows; the built-in ones until the config is applied.
    pub fn kinds(&self) -> &Arc<KindCatalog> {
        &self.kinds
    }

    pub fn set_kinds(&mut self, kinds: Arc<KindCatalog>) {
        self.kinds = kinds;
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
    if let Some(stored) = store.load_protected()? {
        return IndexKey::from_stored(&stored);
    }
    let key = IndexKey::generate()?;
    store_and_verify(store, &key)?;
    Ok(key)
}

fn store_and_verify(store: &dyn KeyStore, key: &IndexKey) -> Result<(), IndexError> {
    store.store_new(key.as_bytes())?;
    verify_stored(store, key)
}

fn verify_stored(store: &dyn KeyStore, key: &IndexKey) -> Result<(), IndexError> {
    let read_back = store
        .load_protected()?
        .ok_or(IndexError::KeyStoreVerificationFailed)?;
    if read_back.as_slice() == key.as_bytes() {
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

/// A creation that failed after the file appeared leaves nothing behind, so
/// the next try is not stopped by `AlreadyExists` and no key-less or
/// half-migrated file is mistaken for a database. The caller's connection is
/// already dropped; the original error is kept.
fn discard_half_created(db_path: &Path, err: IndexError) -> IndexError {
    let sidecars = SIDECAR_SUFFIXES
        .iter()
        .filter_map(|suffix| path_with_suffix(db_path, suffix).ok());
    for path in std::iter::once(db_path.to_path_buf()).chain(sidecars) {
        let _best_effort = remove_file_if_present(&path);
    }
    err
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
        (copy_text, raw_key_text(key).as_str()),
    )?;
    conn.execute_batch("PRAGMA encrypted.cipher_compatibility = 4;")?;
    conn.query_row("SELECT sqlcipher_export('encrypted')", [], |_| Ok(()))?;
    conn.execute_batch(&format!("PRAGMA encrypted.user_version = {user_version};"))?;
    conn.execute_batch("DETACH DATABASE encrypted;")?;
    conn.close().map_err(|(_, err)| err)?;
    Ok(())
}

/// Only unlocks and checks the copy: opening it as an `Index` would migrate it
/// and leave a `.encrypting.before-vN` backup that nothing ever cleans up.
fn check_copy_opens(copy_path: &Path, key: &IndexKey) -> Result<(), IndexError> {
    if starts_with_plaintext_header(copy_path)? {
        return Err(IndexError::NotEncrypted);
    }
    let conn = Connection::open(copy_path)?;
    unlock(&conn, key)?;
    let verdict: String = conn.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    if verdict == "ok" {
        Ok(())
    } else {
        Err(crate::model::corrupt("encrypted copy", &verdict))
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
fn raw_key_text(key: &IndexKey) -> Zeroizing<String> {
    Zeroizing::new(format!("x'{}'", *key.to_hex()))
}

/// A plain index keeps removed text readable in free pages unless they are
/// overwritten; an encrypted file has its free pages encrypted as well.
fn overwrite_deleted_pages(conn: &Connection) -> Result<(), IndexError> {
    conn.execute_batch("PRAGMA secure_delete = ON;")?;
    Ok(())
}

fn unlock(conn: &Connection, key: &IndexKey) -> Result<(), IndexError> {
    let statement = Zeroizing::new(format!("PRAGMA key = \"{}\";", *raw_key_text(key)));
    conn.execute_batch(&statement)?;
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
    run_migrations(conn, migrations)?;
    create_folder_pick_table(conn)
}

#[cfg(test)]
mod tests;
