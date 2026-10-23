use std::path::Path;
use std::sync::Mutex;

use crate::error::KeyStoreError;

const SERVICE_NAME: &str = "ncrs";
const ACCOUNT_NAME: &str = "index-key";

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
    account: String,
}

impl KeyringKeyStore {
    pub fn new() -> Self {
        Self::with_service(SERVICE_NAME)
    }

    /// A different service name keeps tests away from the real entry.
    pub fn with_service(service: &str) -> Self {
        Self {
            service: service.to_owned(),
            account: ACCOUNT_NAME.to_owned(),
        }
    }

    /// One entry per index: two portable copies, or a portable copy and an
    /// installed one, must not overwrite each other's key.
    pub fn with_account(mut self, account: &str) -> Self {
        account.clone_into(&mut self.account);
        self
    }

    /// The store of a portable copy, keyed by its data directory. The
    /// installed layout keeps the plain account, so existing keys still match.
    pub fn portable(data_dir: &Path) -> Self {
        Self::new().with_account(&portable_account(data_dir))
    }

    fn entry(&self) -> Result<keyring::Entry, KeyStoreError> {
        keyring::Entry::new(&self.service, &self.account).map_err(unavailable)
    }
}

// Stored keys are found by this exact string; changing how the path is formed
// strands every existing portable key.
fn portable_account(data_dir: &Path) -> String {
    format!("{ACCOUNT_NAME}:{}", data_dir.display())
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
        match self.entry()?.get_secret() {
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
        self.entry()?.set_secret(key).map_err(unavailable)
    }

    fn replace(&self, key: &[u8]) -> Result<(), KeyStoreError> {
        self.entry()?.set_secret(key).map_err(unavailable)
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

        store.entry().unwrap().delete_credential().unwrap();
        assert_eq!(store.load().unwrap(), None);
    }

    #[test]
    fn portable_account_differs_per_data_dir_and_from_the_installed_one() {
        let first = portable_account(Path::new("/home/user/a/data"));
        let second = portable_account(Path::new("/home/user/b/data"));
        assert_eq!(first, "index-key:/home/user/a/data");
        assert_ne!(first, second);
        assert_ne!(first, ACCOUNT_NAME);
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

        first.entry().unwrap().delete_credential().unwrap();
        second.entry().unwrap().delete_credential().unwrap();
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
