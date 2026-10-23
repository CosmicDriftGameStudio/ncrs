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

    /// `None` only for a folder account without an ID file when `create` is false.
    fn entry(&self, create: bool) -> Result<Option<keyring::Entry>, KeyStoreError> {
        let account = match &self.account {
            Account::Fixed(account) => account.clone(),
            Account::InFolder(id_file) => match folder_account(id_file, create)? {
                Some(account) => account,
                None => return Ok(None),
            },
        };
        keyring::Entry::new(&self.service, &account)
            .map(Some)
            .map_err(unavailable)
    }
}

// Stored keys are found by this exact string; changing how it is formed
// strands every existing entry.
//
// A file that holds nothing but whitespace or NULs counts as missing: a crash
// while writing can leave one behind, and no key can be stored under an empty ID.
fn folder_account(id_file: &Path, create: bool) -> Result<Option<String>, KeyStoreError> {
    loop {
        let content = match std::fs::read(id_file) {
            Ok(bytes) => Some(String::from_utf8_lossy(&bytes).into_owned()),
            Err(err) if err.kind() == ErrorKind::NotFound => None,
            Err(err) => return Err(id_file_error(id_file, &err.to_string())),
        };
        let blank = content.as_deref().is_none_or(|text| {
            text.trim_matches(|c: char| c.is_whitespace() || c == '\0')
                .is_empty()
        });
        match content {
            Some(text) if !blank => return parse_key_id(&text, id_file).map(Some),
            _ if !create => return Ok(None),
            existing => {
                write_new_key_id(id_file, existing.is_some())?;
            }
        }
    }
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

/// A missing file is created exclusively; a blank one is overwritten. Losing
/// a race to another creator is fine: the caller reads the file again.
fn write_new_key_id(id_file: &Path, overwrite_blank: bool) -> Result<(), KeyStoreError> {
    let mut bytes: [u8; KEY_ID_BYTES] = [0; KEY_ID_BYTES];
    getrandom::fill(&mut bytes)
        .map_err(|err| id_file_error(id_file, &format!("no random ID available: {err}")))?;
    let mut line = String::with_capacity(KEY_ID_BYTES * 2 + 1);
    for byte in bytes {
        write!(line, "{byte:02x}").map_err(|err| id_file_error(id_file, &err.to_string()))?;
    }
    line.push('\n');

    let mut options = OpenOptions::new();
    if overwrite_blank {
        options.write(true).truncate(true);
    } else {
        options.write(true).create_new(true);
    }
    match options.open(id_file) {
        Ok(mut file) => file
            .write_all(line.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|err| id_file_error(id_file, &err.to_string())),
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
        let Some(entry) = self.entry(false)? else {
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
}

impl KeyringKeyStore {
    fn set_secret(&self, key: &[u8]) -> Result<(), KeyStoreError> {
        match self.entry(true)? {
            Some(entry) => entry.set_secret(key).map_err(unavailable),
            None => Err(KeyStoreError::Unavailable(
                "no account for the key".to_owned(),
            )),
        }
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

        store
            .entry(false)
            .unwrap()
            .unwrap()
            .delete_credential()
            .unwrap();
        assert_eq!(store.load().unwrap(), None);
    }

    fn id_file_in(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join(KEY_ID_FILE)
    }

    #[test]
    fn missing_id_file_without_create_gives_no_account_and_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let id_file = id_file_in(&dir);
        assert_eq!(folder_account(&id_file, false).unwrap(), None);
        assert!(!id_file.exists());
    }

    #[test]
    fn create_writes_a_hex_id_and_reuses_it() {
        let dir = tempfile::tempdir().unwrap();
        let id_file = id_file_in(&dir);
        let first = folder_account(&id_file, true).unwrap().unwrap();
        let written = std::fs::read_to_string(&id_file).unwrap();
        let id = written.trim();
        assert_eq!(id.len(), 32);
        assert!(id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
        assert_eq!(first, format!("{ACCOUNT_NAME}:{id}"));
        assert_eq!(folder_account(&id_file, true).unwrap().unwrap(), first);
        assert_eq!(folder_account(&id_file, false).unwrap().unwrap(), first);
    }

    #[test]
    fn folders_get_different_accounts_that_differ_from_the_installed_one() {
        let first_dir = tempfile::tempdir().unwrap();
        let second_dir = tempfile::tempdir().unwrap();
        let first = folder_account(&id_file_in(&first_dir), true)
            .unwrap()
            .unwrap();
        let second = folder_account(&id_file_in(&second_dir), true)
            .unwrap()
            .unwrap();
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
            assert_eq!(folder_account(&id_file, false).unwrap(), None);
            let account = folder_account(&id_file, true).unwrap().unwrap();
            assert_eq!(account.len(), ACCOUNT_NAME.len() + 1 + 32);
            assert_eq!(folder_account(&id_file, false).unwrap().unwrap(), account);
        }
    }

    #[test]
    fn id_with_trailing_newline_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let id_file = id_file_in(&dir);
        std::fs::write(&id_file, "0123456789abcdef0123456789abcdef\r\n").unwrap();
        assert_eq!(
            folder_account(&id_file, false).unwrap().unwrap(),
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
            for create in [false, true] {
                let err = folder_account(&id_file, create).unwrap_err();
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
            .entry(false)
            .unwrap()
            .unwrap()
            .delete_credential()
            .unwrap();
        second
            .entry(false)
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
}
