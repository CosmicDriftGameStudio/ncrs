//! The passwords of saved servers, one keychain entry per account, and the
//! passphrases of key files, one entry per absolute path. A secret is only ever
//! written to the keychain, never to a file or a log, and only after the user
//! ticked "remember". The calls block; the app makes them on the blocking pool.

use std::fmt;
use std::path::Path;

use zeroize::Zeroizing;

use super::{Secret, ServerId};

/// The keychain service of a normal install, and of the App Store build, whose
/// entries must stay apart from it (see the key of the index).
const SERVICE: &str = "ncrs-sftp";
const APP_STORE_SERVICE: &str = "ncrs-appstore-sftp";

/// The keychain service for a build: the App Store build keeps its entries
/// apart from those of an installed ncrs.
fn service_for(sandboxed: bool) -> &'static str {
    if sandboxed {
        APP_STORE_SERVICE
    } else {
        SERVICE
    }
}

/// Where passwords are kept. A trait so tests never reach the real keychain.
/// The errors are text for the status line and never contain a password.
pub trait PasswordStore: Send + Sync + fmt::Debug {
    fn get(&self, server: &ServerId) -> Result<Option<Secret>, String>;
    fn set(&self, server: &ServerId, password: &Secret) -> Result<(), String>;
    /// Removes the entry; none there is not an error.
    fn forget(&self, server: &ServerId) -> Result<(), String>;
}

/// The operating system's credential store.
#[derive(Debug)]
pub struct KeyringPasswords {
    service: String,
}

impl KeyringPasswords {
    pub fn new() -> Self {
        Self::with_service(service_for(crate::sandbox_active()))
    }

    /// A different service name keeps tests away from the real entries.
    pub fn with_service(service: &str) -> Self {
        Self {
            service: service.to_owned(),
        }
    }

    /// The service and the account an entry of `server` is stored under.
    fn names(&self, server: &ServerId) -> (&str, String) {
        (&self.service, server.account())
    }

    fn entry(&self, server: &ServerId) -> Result<keyring::Entry, String> {
        crate::forbid_user_machine_in_tests("the system keychain for SFTP passwords");
        let (service, account) = self.names(server);
        keyring::Entry::new(service, &account).map_err(|error| error.to_string())
    }
}

impl Default for KeyringPasswords {
    fn default() -> Self {
        Self::new()
    }
}

impl PasswordStore for KeyringPasswords {
    fn get(&self, server: &ServerId) -> Result<Option<Secret>, String> {
        match self.entry(server)?.get_secret() {
            Ok(stored) => {
                let wiped = Zeroizing::new(stored);
                let text = std::str::from_utf8(&wiped)
                    .map_err(|_not_text| "the entry is not text".to_owned())?;
                Ok(Some(Secret::new(text.to_owned())))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    fn set(&self, server: &ServerId, password: &Secret) -> Result<(), String> {
        self.entry(server)?
            .set_secret(password.expose().as_bytes())
            .map_err(|error| error.to_string())
    }

    fn forget(&self, server: &ServerId) -> Result<(), String> {
        match self.entry(server)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

/// Where the passphrases of key files are kept, by the absolute path of the
/// key. A trait so tests never reach the real keychain. The errors are text and
/// never contain a passphrase.
pub trait PassphraseStore: Send + Sync + fmt::Debug {
    fn get(&self, key_file: &Path) -> Result<Option<Secret>, String>;
    fn set(&self, key_file: &Path, passphrase: &Secret) -> Result<(), String>;
    /// Removes the entry; none there is not an error.
    fn forget(&self, key_file: &Path) -> Result<(), String>;
}

/// The account of a key file's passphrase. A password account is
/// `user@host:port` and a user name has no `:`, so the prefix keeps the two
/// apart whatever the path holds.
fn passphrase_account(key_file: &Path) -> String {
    format!("key-file:{}", key_file.display())
}

/// The operating system's credential store, for passphrases.
#[derive(Debug)]
pub struct KeyringPassphrases {
    service: String,
}

impl KeyringPassphrases {
    pub fn new() -> Self {
        Self::with_service(service_for(crate::sandbox_active()))
    }

    pub fn with_service(service: &str) -> Self {
        Self {
            service: service.to_owned(),
        }
    }

    fn entry(&self, key_file: &Path) -> Result<keyring::Entry, String> {
        crate::forbid_user_machine_in_tests("the system keychain for key passphrases");
        keyring::Entry::new(&self.service, &passphrase_account(key_file))
            .map_err(|error| error.to_string())
    }
}

impl Default for KeyringPassphrases {
    fn default() -> Self {
        Self::new()
    }
}

impl PassphraseStore for KeyringPassphrases {
    fn get(&self, key_file: &Path) -> Result<Option<Secret>, String> {
        match self.entry(key_file)?.get_secret() {
            Ok(stored) => {
                let wiped = Zeroizing::new(stored);
                let text = std::str::from_utf8(&wiped)
                    .map_err(|_not_text| "the entry is not text".to_owned())?;
                Ok(Some(Secret::new(text.to_owned())))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    fn set(&self, key_file: &Path, passphrase: &Secret) -> Result<(), String> {
        self.entry(key_file)?
            .set_secret(passphrase.expose().as_bytes())
            .map_err(|error| error.to_string())
    }

    fn forget(&self, key_file: &Path) -> Result<(), String> {
        match self.entry(key_file)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

/// Passphrases in memory, for tests. Prints no content.
#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
pub struct InMemoryPassphrases {
    entries: std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, Secret>>,
}

#[cfg(any(test, feature = "test-support"))]
impl fmt::Debug for InMemoryPassphrases {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InMemoryPassphrases")
    }
}

#[cfg(any(test, feature = "test-support"))]
impl InMemoryPassphrases {
    fn entries(
        &self,
    ) -> std::sync::MutexGuard<'_, std::collections::HashMap<std::path::PathBuf, Secret>> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(any(test, feature = "test-support"))]
impl PassphraseStore for InMemoryPassphrases {
    fn get(&self, key_file: &Path) -> Result<Option<Secret>, String> {
        Ok(self.entries().get(key_file).cloned())
    }

    fn set(&self, key_file: &Path, passphrase: &Secret) -> Result<(), String> {
        self.entries()
            .insert(key_file.to_path_buf(), passphrase.clone());
        Ok(())
    }

    fn forget(&self, key_file: &Path) -> Result<(), String> {
        self.entries().remove(key_file);
        Ok(())
    }
}

/// Passwords in memory, for tests. Prints no content.
#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
pub struct InMemoryPasswords {
    entries: std::sync::Mutex<std::collections::HashMap<ServerId, Secret>>,
}

#[cfg(any(test, feature = "test-support"))]
impl fmt::Debug for InMemoryPasswords {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InMemoryPasswords")
    }
}

#[cfg(any(test, feature = "test-support"))]
impl InMemoryPasswords {
    pub fn with(server: &ServerId, password: &str) -> Self {
        let store = Self::default();
        store
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(server.clone(), Secret::new(password.to_owned()));
        store
    }
}

#[cfg(any(test, feature = "test-support"))]
impl PasswordStore for InMemoryPasswords {
    fn get(&self, server: &ServerId) -> Result<Option<Secret>, String> {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(entries.get(server).cloned())
    }

    fn set(&self, server: &ServerId, password: &Secret) -> Result<(), String> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(server.clone(), password.clone());
        Ok(())
    }

    fn forget(&self, server: &ServerId) -> Result<(), String> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(server);
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

    #[test]
    fn in_memory_store_round_trip() {
        let server = ServerId::new("marc", "example.org", 22);
        let other = ServerId::new("marc", "example.org", 2222);
        let store = InMemoryPasswords::default();
        assert_eq!(store.get(&server).unwrap(), None);

        store.set(&server, &Secret::new("pw".to_owned())).unwrap();
        assert_eq!(store.get(&server).unwrap().unwrap().expose(), "pw");
        assert_eq!(store.get(&other).unwrap(), None, "one entry per server");

        store.forget(&server).unwrap();
        store.forget(&server).unwrap();
        assert_eq!(store.get(&server).unwrap(), None);
    }

    #[test]
    fn the_two_builds_use_two_services_and_one_entry_per_account() {
        assert_eq!(service_for(false), "ncrs-sftp");
        assert_eq!(service_for(true), "ncrs-appstore-sftp");
        let normal = KeyringPasswords::with_service(service_for(false));
        let store = KeyringPasswords::with_service(service_for(true));
        let work = ServerId::new("marc", "Files.Example.org", 2222);
        let home = ServerId::new("marc", "files.example.org", 22);
        assert_eq!(
            normal.names(&work),
            ("ncrs-sftp", "marc@files.example.org:2222".to_owned())
        );
        assert_eq!(store.names(&work).0, "ncrs-appstore-sftp");
        assert_ne!(
            store.names(&work),
            store.names(&home),
            "another port is another entry"
        );
        assert_eq!(store.names(&home).1, "marc@files.example.org:22");
    }

    #[test]
    fn a_passphrase_account_cannot_be_a_password_account() {
        let account = passphrase_account(Path::new("/home/marc/.ssh/a@b:22"));
        assert_eq!(account, "key-file:/home/marc/.ssh/a@b:22");
        assert_ne!(account, ServerId::new("key-file", "x", 22).account());
    }

    #[test]
    fn in_memory_passphrases_round_trip_per_path() {
        let store = InMemoryPassphrases::default();
        let key = Path::new("/k/a");
        assert_eq!(store.get(key).unwrap(), None);
        store.set(key, &Secret::new("pp".to_owned())).unwrap();
        assert_eq!(store.get(key).unwrap().unwrap().expose(), "pp");
        assert_eq!(store.get(Path::new("/k/b")).unwrap(), None);
        store.forget(key).unwrap();
        assert_eq!(store.get(key).unwrap(), None);
    }

    #[test]
    fn a_store_never_shows_what_it_holds() {
        let server = ServerId::new("marc", "example.org", 22);
        let store = InMemoryPasswords::with(&server, "hunter2");
        assert!(!format!("{store:?}").contains("hunter2"));
    }
}
