//! The passwords of saved servers, one keychain entry per account. A password
//! is only ever written to the keychain, never to a file or a log, and only
//! after the user ticked "remember". The calls block; the app makes them on
//! the blocking pool.

use std::fmt;

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
        Self::with_service(service_for(crate::fs::sandbox_active()))
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
        crate::config::forbid_user_machine_in_tests("the system keychain for SFTP passwords");
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

/// Passwords in memory, for tests. Prints no content.
#[cfg(test)]
#[derive(Default)]
pub struct InMemoryPasswords {
    entries: std::sync::Mutex<std::collections::HashMap<ServerId, Secret>>,
}

#[cfg(test)]
impl fmt::Debug for InMemoryPasswords {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InMemoryPasswords")
    }
}

#[cfg(test)]
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

#[cfg(test)]
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
    fn a_store_never_shows_what_it_holds() {
        let server = ServerId::new("marc", "example.org", 22);
        let store = InMemoryPasswords::with(&server, "hunter2");
        assert!(!format!("{store:?}").contains("hunter2"));
    }
}
