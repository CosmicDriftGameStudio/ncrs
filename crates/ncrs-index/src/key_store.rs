use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::error::KeyStoreError;

const SERVICE_NAME: &str = "ncrs";
const ACCOUNT_NAME: &str = "index-key";
const KEY_ID_FILE: &str = "key-id";
const KEY_ID_BYTES: usize = 16;

pub trait KeyStore {
    fn load(&self) -> Result<Option<Vec<u8>>, KeyStoreError>;

    /// Must fail with `AlreadyExists` instead of replacing an existing entry:
    /// the old key may be the only way to read a database.
    fn store_new(&self, key: &[u8]) -> Result<(), KeyStoreError>;

    /// The single exception to `store_new`'s rule: overwrites unconditionally.
    /// Called only from `Index::replace_key`, after the new key has proven
    /// that it opens the database.
    fn replace(&self, key: &[u8]) -> Result<(), KeyStoreError>;

    /// Removes this store's own entry; no entry is not an error. Only for an
    /// index the user started over or a key made for an encryption that did
    /// not happen.
    fn delete(&self) -> Result<(), KeyStoreError>;
}

/// The operating system's credential store: Keychain, Credential Manager or
/// Secret Service.
#[derive(Debug)]
pub struct KeyringKeyStore {
    service: String,
    account: Account,
}

#[derive(Debug)]
enum Account {
    Fixed(String),
    /// Named by a random ID kept in this file, so the entry survives moving
    /// the folder.
    InFolder(PathBuf),
}

impl KeyringKeyStore {
    pub fn new() -> Self {
        Self::with_service(SERVICE_NAME)
    }

    /// A different service name keeps tests away from the real entry.
    pub fn with_service(service: &str) -> Self {
        Self {
            service: service.to_owned(),
            account: Account::Fixed(ACCOUNT_NAME.to_owned()),
        }
    }

    /// One entry per index: two portable copies, or a portable copy and an
    /// installed one, must not overwrite each other's key.
    pub fn with_account(mut self, account: &str) -> Self {
        self.account = Account::Fixed(account.to_owned());
        self
    }

    /// The store of a portable copy, keyed by a random ID in its data
    /// directory. The installed layout keeps the plain account, so existing
    /// keys still match.
    pub fn portable(data_dir: &Path) -> Self {
        Self {
            account: Account::InFolder(data_dir.join(KEY_ID_FILE)),
            ..Self::new()
        }
    }

    /// `None` for a folder account without an ID file: nothing was stored.
    fn existing_entry(&self) -> Result<Option<keyring::Entry>, KeyStoreError> {
        let account = match &self.account {
            Account::Fixed(account) => account.clone(),
            Account::InFolder(id_file) => match existing_folder_account(id_file)? {
                Some(account) => account,
                None => return Ok(None),
            },
        };
        self.entry_of(&account).map(Some)
    }

    /// Creates the ID file of a folder account when it is missing.
    fn entry_for_writing(&self) -> Result<keyring::Entry, KeyStoreError> {
        let account = match &self.account {
            Account::Fixed(account) => account.clone(),
            Account::InFolder(id_file) => created_folder_account(id_file)?,
        };
        self.entry_of(&account)
    }

    fn entry_of(&self, account: &str) -> Result<keyring::Entry, KeyStoreError> {
        keyring::Entry::new(&self.service, account).map_err(unavailable)
    }
}

// Stored keys are found by this exact string; changing how it is formed
// strands every existing entry.
//
// A file that holds nothing but whitespace or NULs counts as missing: a crash
// while writing can leave one behind, and no key can be stored under an empty ID.
fn existing_folder_account(id_file: &Path) -> Result<Option<String>, KeyStoreError> {
    match read_key_id_file(id_file)? {
        KeyIdFile::Id(text) => parse_key_id(&text, id_file).map(Some),
        KeyIdFile::Missing | KeyIdFile::Blank => Ok(None),
    }
}

/// How often a missing or blank ID file is written again before giving up: a
/// file that stays blank after a write is not a race but a broken folder.
const ID_FILE_ATTEMPTS: usize = 5;

fn created_folder_account(id_file: &Path) -> Result<String, KeyStoreError> {
    for _ in 0..ID_FILE_ATTEMPTS {
        match read_key_id_file(id_file)? {
            KeyIdFile::Id(text) => return parse_key_id(&text, id_file),
            KeyIdFile::Missing => write_new_key_id(id_file)?,
            KeyIdFile::Blank => {
                remove_if_still_blank(id_file)?;
                write_new_key_id(id_file)?;
            }
        }
    }
    Err(id_file_error(id_file, "could not create a stable ID"))
}

/// A blank file is removed, never cut short and written into: whoever sees
/// it blank while another creator is writing would overwrite that creator's ID.
/// Reading and removing are two steps, so the file is first moved to a name of
/// its own: what the move caught is looked at there, and an ID that a creator
/// finished writing in the meantime is linked back.
fn remove_if_still_blank(id_file: &Path) -> Result<(), KeyStoreError> {
    let mut caught_name = id_file.as_os_str().to_owned();
    caught_name.push(format!(
        ".{}-{}.blank",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos())
    ));
    let caught = PathBuf::from(caught_name);
    match std::fs::rename(id_file, &caught) {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(id_file_error(id_file, &err.to_string())),
    }
    let put_back = match read_key_id_file(&caught)? {
        KeyIdFile::Id(text) => restore_key_id(&caught, id_file, &text),
        KeyIdFile::Missing | KeyIdFile::Blank => Ok(()),
    };
    match std::fs::remove_file(&caught) {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => return Err(id_file_error(&caught, &err.to_string())),
    }
    put_back
}

/// Gives an ID back its name unless another creator took the name meanwhile:
/// a link, or where links are not possible an exclusive create and write.
fn restore_key_id(caught: &Path, id_file: &Path, content: &str) -> Result<(), KeyStoreError> {
    match std::fs::hard_link(caught, id_file) {
        Ok(()) => return Ok(()),
        Err(err) if err.kind() == ErrorKind::AlreadyExists => return Ok(()),
        Err(_) => {}
    }
    let created = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(id_file);
    match created {
        Ok(mut file) => file
            .write_all(content.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|err| id_file_error(id_file, &err.to_string())),
        Err(err) if err.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(err) => Err(id_file_error(id_file, &err.to_string())),
    }
}

enum KeyIdFile {
    Missing,
    Blank,
    Id(String),
}

fn read_key_id_file(id_file: &Path) -> Result<KeyIdFile, KeyStoreError> {
    let text = match std::fs::read(id_file) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(KeyIdFile::Missing),
        Err(err) => return Err(id_file_error(id_file, &err.to_string())),
    };
    let blank = text
        .trim_matches(|c: char| c.is_whitespace() || c == '\0')
        .is_empty();
    Ok(if blank {
        KeyIdFile::Blank
    } else {
        KeyIdFile::Id(text)
    })
}

fn parse_key_id(content: &str, id_file: &Path) -> Result<String, KeyStoreError> {
    let id = content.trim();
    let valid = id.len() == KEY_ID_BYTES * 2
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if valid {
        Ok(format!("{ACCOUNT_NAME}:{id}"))
    } else {
        Err(id_file_error(id_file, "it does not contain a valid ID"))
    }
}

/// The ID is written to a file of its own and linked to the name of the ID
/// file, so the name never exists without its content. Losing a race to
/// another creator is fine: the caller reads the file again. Where links are
/// not possible (exFAT, some network shares) the file is created exclusively
/// and written in place.
fn write_new_key_id(id_file: &Path) -> Result<(), KeyStoreError> {
    let mut bytes: [u8; KEY_ID_BYTES] = [0; KEY_ID_BYTES];
    getrandom::fill(&mut bytes)
        .map_err(|err| id_file_error(id_file, &format!("no random ID available: {err}")))?;
    let mut line = String::with_capacity(KEY_ID_BYTES * 2 + 1);
    for byte in bytes {
        write!(line, "{byte:02x}").map_err(|err| id_file_error(id_file, &err.to_string()))?;
    }
    let tag = line.clone();
    line.push('\n');

    let mut staged_name = id_file.as_os_str().to_owned();
    staged_name.push(format!(".{tag}.new"));
    let staged = PathBuf::from(staged_name);
    let outcome = write_synced(&staged, &line).and_then(|()| std::fs::hard_link(&staged, id_file));
    drop(std::fs::remove_file(&staged));
    match outcome {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(_) => write_in_place(id_file, &line),
    }
}

fn write_synced(path: &Path, line: &str) -> std::io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(line.as_bytes())?;
    file.sync_all()
}

fn write_in_place(id_file: &Path, line: &str) -> Result<(), KeyStoreError> {
    match write_synced(id_file, line) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(err) => Err(id_file_error(id_file, &err.to_string())),
    }
}

fn id_file_error(id_file: &Path, reason: &str) -> KeyStoreError {
    KeyStoreError::Unavailable(format!("key ID file {}: {reason}", id_file.display()))
}

impl Default for KeyringKeyStore {
    fn default() -> Self {
        Self::new()
    }
}

fn unavailable(err: keyring::Error) -> KeyStoreError {
    KeyStoreError::Unavailable(err.to_string())
}

impl KeyStore for KeyringKeyStore {
    fn load(&self) -> Result<Option<Vec<u8>>, KeyStoreError> {
        let Some(entry) = self.existing_entry()? else {
            return Ok(None);
        };
        match entry.get_secret() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(unavailable(err)),
        }
    }

    // The credential stores offer no atomic create-if-absent, so this is a
    // check followed by a write. Only one process creates the index key, on
    // the first start, which is the window this leaves open.
    fn store_new(&self, key: &[u8]) -> Result<(), KeyStoreError> {
        if self.load()?.is_some() {
            return Err(KeyStoreError::AlreadyExists);
        }
        self.set_secret(key)
    }

    fn replace(&self, key: &[u8]) -> Result<(), KeyStoreError> {
        self.set_secret(key)
    }

    fn delete(&self) -> Result<(), KeyStoreError> {
        let Some(entry) = self.existing_entry()? else {
            return Ok(());
        };
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(unavailable(err)),
        }
    }
}

impl KeyringKeyStore {
    fn set_secret(&self, key: &[u8]) -> Result<(), KeyStoreError> {
        self.entry_for_writing()?
            .set_secret(key)
            .map_err(unavailable)
    }
}

/// For tests; keeps the key in memory only.
#[derive(Debug, Default)]
pub struct InMemoryKeyStore {
    entry: Mutex<Option<Vec<u8>>>,
}

impl InMemoryKeyStore {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn with_entry(entry: Vec<u8>) -> Self {
        Self {
            entry: Mutex::new(Some(entry)),
        }
    }

    pub fn snapshot(&self) -> Option<Vec<u8>> {
        self.lock().clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Vec<u8>>> {
        self.entry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl KeyStore for InMemoryKeyStore {
    fn load(&self) -> Result<Option<Vec<u8>>, KeyStoreError> {
        Ok(self.snapshot())
    }

    fn store_new(&self, key: &[u8]) -> Result<(), KeyStoreError> {
        let mut entry = self.lock();
        if entry.is_some() {
            return Err(KeyStoreError::AlreadyExists);
        }
        *entry = Some(key.to_vec());
        Ok(())
    }

    fn replace(&self, key: &[u8]) -> Result<(), KeyStoreError> {
        *self.lock() = Some(key.to_vec());
        Ok(())
    }

    fn delete(&self) -> Result<(), KeyStoreError> {
        *self.lock() = None;
        Ok(())
    }
}

/// The key in a file, for a throwaway data folder (`--data-dir`): the key
/// sits next to the index it opens, not in the keychain.
#[derive(Debug)]
pub struct FileKeyStore {
    path: PathBuf,
}

impl FileKeyStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn create(&self, options: &mut OpenOptions, key: &[u8]) -> std::io::Result<()> {
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(options, 0o600);
        options.write(true).open(&self.path)?.write_all(key)
    }

    /// The key goes into a file next to `path` first and is renamed over it,
    /// so an abort leaves the old key whole.
    fn write_over(&self, key: &[u8]) -> std::io::Result<()> {
        let mut name = self.path.as_os_str().to_owned();
        name.push(".new");
        let next = PathBuf::from(name);
        match std::fs::remove_file(&next) {
            Ok(()) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
        let written = self
            .create_synced(&next, key)
            .and_then(|()| std::fs::rename(&next, &self.path));
        if written.is_err() {
            drop(std::fs::remove_file(&next));
        }
        written
    }

    fn create_synced(&self, next: &Path, key: &[u8]) -> std::io::Result<()> {
        let mut options = OpenOptions::new();
        options.create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.write(true).open(next)?;
        file.write_all(key)?;
        file.sync_all()
    }
}

impl KeyStore for FileKeyStore {
    fn load(&self) -> Result<Option<Vec<u8>>, KeyStoreError> {
        match std::fs::read(&self.path) {
            Ok(key) => Ok(Some(key)),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(KeyStoreError::Unavailable(err.to_string())),
        }
    }

    fn store_new(&self, key: &[u8]) -> Result<(), KeyStoreError> {
        match self.create(OpenOptions::new().create_new(true), key) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == ErrorKind::AlreadyExists => Err(KeyStoreError::AlreadyExists),
            Err(err) => Err(KeyStoreError::Unavailable(err.to_string())),
        }
    }

    fn replace(&self, key: &[u8]) -> Result<(), KeyStoreError> {
        self.write_over(key)
            .map_err(|err| KeyStoreError::Unavailable(err.to_string()))
    }

    fn delete(&self) -> Result<(), KeyStoreError> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
            Err(err) => Err(KeyStoreError::Unavailable(err.to_string())),
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    /// Talks to the real credential store; run with `--ignored`. It is the
    /// only place that deletes an entry, and only under a throwaway service
    /// name. Multi-thread because the app calls from inside a tokio context,
    /// where a store that starts its own runtime would panic.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "uses the real operating system credential store"]
    async fn real_store_creates_reads_and_refuses_to_overwrite() {
        let service = format!("ncrs-test-{}", std::process::id());
        let store = KeyringKeyStore::with_service(&service);
        assert_eq!(store.load().unwrap(), None);

        let key: [u8; 32] = [0x5a; 32];
        store.store_new(&key).unwrap();
        assert_eq!(store.load().unwrap().unwrap(), key);
        assert_eq!(store.store_new(&[1; 32]), Err(KeyStoreError::AlreadyExists));
        assert_eq!(store.load().unwrap().unwrap(), key);

        let replacement: [u8; 32] = [0x6b; 32];
        store.replace(&replacement).unwrap();
        assert_eq!(store.load().unwrap().unwrap(), replacement);

        store.delete().unwrap();
        assert_eq!(store.load().unwrap(), None);
        store.delete().unwrap();
    }

    #[test]
    fn deleting_the_memory_store_entry_empties_it_and_never_fails() {
        let store = InMemoryKeyStore::with_entry(vec![1; 32]);

        store.delete().unwrap();
        assert_eq!(store.load().unwrap(), None);
        store.delete().unwrap();
    }

    fn id_file_in(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join(KEY_ID_FILE)
    }

    #[test]
    fn missing_id_file_without_create_gives_no_account_and_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let id_file = id_file_in(&dir);
        assert_eq!(existing_folder_account(&id_file).unwrap(), None);
        assert!(!id_file.exists());
    }

    #[test]
    fn create_writes_a_hex_id_and_reuses_it() {
        let dir = tempfile::tempdir().unwrap();
        let id_file = id_file_in(&dir);
        let first = created_folder_account(&id_file).unwrap();
        let written = std::fs::read_to_string(&id_file).unwrap();
        let id = written.trim();
        assert_eq!(id.len(), 32);
        assert!(id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
        assert_eq!(first, format!("{ACCOUNT_NAME}:{id}"));
        assert_eq!(created_folder_account(&id_file).unwrap(), first);
        assert_eq!(existing_folder_account(&id_file).unwrap().unwrap(), first);
    }

    #[test]
    fn folders_get_different_accounts_that_differ_from_the_installed_one() {
        let first_dir = tempfile::tempdir().unwrap();
        let second_dir = tempfile::tempdir().unwrap();
        let first = created_folder_account(&id_file_in(&first_dir)).unwrap();
        let second = created_folder_account(&id_file_in(&second_dir)).unwrap();
        assert_ne!(first, second);
        assert_ne!(first, ACCOUNT_NAME);
        assert_ne!(second, ACCOUNT_NAME);
    }

    #[test]
    fn blank_id_file_counts_as_missing_and_is_overwritten_on_create() {
        let dir = tempfile::tempdir().unwrap();
        let id_file = id_file_in(&dir);
        for content in [&b""[..], b"\n", b"\0\0\0\0"] {
            std::fs::write(&id_file, content).unwrap();
            assert_eq!(existing_folder_account(&id_file).unwrap(), None);
            let account = created_folder_account(&id_file).unwrap();
            assert_eq!(account.len(), ACCOUNT_NAME.len() + 1 + 32);
            assert_eq!(existing_folder_account(&id_file).unwrap().unwrap(), account);
        }
    }

    #[test]
    fn creators_at_the_same_time_end_up_with_one_account() {
        use std::sync::{Arc, Barrier};
        for round in 0..40 {
            let dir = tempfile::tempdir().unwrap();
            let id_file = id_file_in(&dir);
            let start = Arc::new(Barrier::new(4));
            let creators: Vec<_> = std::iter::repeat_with(|| {
                let (file, gate) = (id_file.clone(), Arc::clone(&start));
                std::thread::spawn(move || {
                    gate.wait();
                    created_folder_account(&file).unwrap()
                })
            })
            .take(4)
            .collect();
            let accounts: Vec<String> = creators.into_iter().map(|c| c.join().unwrap()).collect();

            let kept = existing_folder_account(&id_file).unwrap().unwrap();
            assert!(
                accounts.iter().all(|account| *account == kept),
                "round {round}"
            );
            let leftovers = std::fs::read_dir(dir.path()).unwrap().count();
            assert_eq!(leftovers, 1, "no staging file stays behind");
        }
    }

    #[test]
    fn an_id_that_arrived_after_the_blank_was_seen_stays() {
        let dir = tempfile::tempdir().unwrap();
        let id_file = id_file_in(&dir);
        std::fs::write(&id_file, "0123456789abcdef0123456789abcdef\n").unwrap();

        remove_if_still_blank(&id_file).unwrap();

        assert_eq!(
            std::fs::read_to_string(&id_file).unwrap(),
            "0123456789abcdef0123456789abcdef\n"
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_blank_file_is_removed_without_leftovers() {
        let dir = tempfile::tempdir().unwrap();
        let id_file = id_file_in(&dir);
        std::fs::write(&id_file, "\n\0 ").unwrap();

        remove_if_still_blank(&id_file).unwrap();

        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn id_with_trailing_newline_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let id_file = id_file_in(&dir);
        std::fs::write(&id_file, "0123456789abcdef0123456789abcdef\r\n").unwrap();
        assert_eq!(
            existing_folder_account(&id_file).unwrap().unwrap(),
            "index-key:0123456789abcdef0123456789abcdef"
        );
    }

    #[test]
    fn invalid_id_content_is_an_error_without_echoing_it() {
        let dir = tempfile::tempdir().unwrap();
        let id_file = id_file_in(&dir);
        for content in [
            "short",
            "0123456789ABCDEF0123456789ABCDEF",
            "g123456789abcdef0123456789abcdef",
        ] {
            std::fs::write(&id_file, content).unwrap();
            for err in [
                existing_folder_account(&id_file).unwrap_err(),
                created_folder_account(&id_file).unwrap_err(),
            ] {
                assert!(err.to_string().contains(KEY_ID_FILE));
                assert!(!err.to_string().contains(content));
            }
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "uses the real operating system credential store"]
    async fn stores_with_different_accounts_do_not_see_each_other() {
        let service = format!("ncrs-test-accounts-{}", std::process::id());
        let first = KeyringKeyStore::with_service(&service).with_account("one");
        let second = KeyringKeyStore::with_service(&service).with_account("two");

        first.store_new(&[7; 32]).unwrap();
        assert_eq!(second.load().unwrap(), None);
        second.store_new(&[8; 32]).unwrap();
        assert_eq!(first.load().unwrap().unwrap(), [7; 32]);

        first
            .existing_entry()
            .unwrap()
            .unwrap()
            .delete_credential()
            .unwrap();
        second
            .existing_entry()
            .unwrap()
            .unwrap()
            .delete_credential()
            .unwrap();
    }

    #[test]
    fn in_memory_replace_overwrites_an_entry() {
        let store = InMemoryKeyStore::with_entry(vec![1; 32]);
        store.replace(&[2; 32]).unwrap();
        assert_eq!(store.snapshot().unwrap(), vec![2; 32]);

        let empty = InMemoryKeyStore::empty();
        empty.replace(&[3; 32]).unwrap();
        assert_eq!(empty.snapshot().unwrap(), vec![3; 32]);
    }

    #[test]
    fn the_file_store_follows_the_key_store_contract() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileKeyStore::new(dir.path().join("index.key"));
        assert_eq!(store.load().unwrap(), None);

        store.store_new(&[1; 32]).unwrap();
        assert_eq!(store.load().unwrap(), Some(vec![1; 32]));
        assert_eq!(store.store_new(&[2; 32]), Err(KeyStoreError::AlreadyExists));
        assert_eq!(store.load().unwrap(), Some(vec![1; 32]));

        store.replace(&[3; 8]).unwrap();
        assert_eq!(store.load().unwrap(), Some(vec![3; 8]));

        store.delete().unwrap();
        assert_eq!(store.load().unwrap(), None);
        store.delete().unwrap();
    }

    #[test]
    fn replacing_the_key_leaves_no_second_file_and_keeps_the_new_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.key");
        let store = FileKeyStore::new(path.clone());
        store.store_new(&[1; 32]).unwrap();

        store.replace(&[2; 32]).unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), vec![2; 32]);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn a_replaced_key_file_is_private() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.key");
        let store = FileKeyStore::new(path.clone());
        store.store_new(&[1; 32]).unwrap();

        store.replace(&[2; 32]).unwrap();

        let mode = std::fs::metadata(path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn the_key_file_is_private() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.key");
        FileKeyStore::new(path.clone()).store_new(&[1; 32]).unwrap();
        let mode = std::fs::metadata(path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
