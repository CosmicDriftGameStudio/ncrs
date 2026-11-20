//! Where the key of the encrypted notes is kept, and whether it is in memory.
//!
//! The key lives in the credential store of the system (Keychain, Secret
//! Service, Credential Manager). "Unlocked" means a copy is in memory; locking
//! drops it. The store is a trait so the tests never reach the real one.

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use zeroize::Zeroizing;

use crate::crypt::NoteKey;

const SERVICE: &str = "ncrs";
const ACCOUNT: &str = "notes-key";

/// Where the one key of the notes is kept between runs.
pub trait KeyStore: fmt::Debug + Send + Sync {
    /// `None` when no key was stored yet. May block on a system dialog.
    fn load(&self) -> Result<Option<NoteKey>, String>;
    /// Replaces the stored key.
    fn store(&self, key: &NoteKey) -> Result<(), String>;
}

/// The credential store of the operating system.
#[derive(Debug, Default, Clone, Copy)]
pub struct KeyringKeyStore;

impl KeyringKeyStore {
    fn entry() -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, ACCOUNT).map_err(|err| err.to_string())
    }
}

impl KeyStore for KeyringKeyStore {
    fn load(&self) -> Result<Option<NoteKey>, String> {
        match Self::entry()?.get_secret() {
            Ok(secret) => NoteKey::from_bytes(&Zeroizing::new(secret))
                .map(Some)
                .ok_or_else(|| "the stored key has the wrong length".to_owned()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(err.to_string()),
        }
    }

    fn store(&self, key: &NoteKey) -> Result<(), String> {
        Self::entry()?
            .set_secret(key.as_bytes())
            .map_err(|err| err.to_string())
    }
}

/// A key store in memory, for the tests and for a build without a credential
/// store. Clones share the key.
#[derive(Debug, Default, Clone)]
pub struct MemoryKeyStore {
    key: Arc<Mutex<Option<NoteKey>>>,
    /// When set, every call fails with this reason.
    fail_with: Arc<Mutex<Option<String>>>,
}

impl MemoryKeyStore {
    pub fn failing(&self, reason: Option<&str>) {
        *self
            .fail_with
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = reason.map(str::to_owned);
    }

    fn check(&self) -> Result<(), String> {
        match &*self
            .fail_with
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
        {
            Some(reason) => Err(reason.clone()),
            None => Ok(()),
        }
    }
}

impl KeyStore for MemoryKeyStore {
    fn load(&self) -> Result<Option<NoteKey>, String> {
        self.check()?;
        Ok(self
            .key
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone())
    }

    fn store(&self, key: &NoteKey) -> Result<(), String> {
        self.check()?;
        *self.key.lock().unwrap_or_else(PoisonError::into_inner) = Some(key.clone());
        Ok(())
    }
}

/// The key of the stored one, made and stored first when there is none and
/// `create` is set. Blocking: run it off the UI thread.
pub fn obtain_key(store: &dyn KeyStore, create: bool) -> Result<Option<NoteKey>, String> {
    if let Some(key) = store.load()? {
        return Ok(Some(key));
    }
    if !create {
        return Ok(None);
    }
    let key = NoteKey::generate().map_err(|err| err.to_string())?;
    store.store(&key)?;
    Ok(Some(key))
}

/// Stores the key a recovery text names, on a computer that has none. A
/// different key already there is not replaced: the notes sealed with it
/// would be lost.
pub fn restore_key(store: &dyn KeyStore, recovery_text: &str) -> Result<NoteKey, String> {
    let key = NoteKey::from_recovery_text(recovery_text).map_err(|err| err.to_string())?;
    match store.load()? {
        Some(existing) if existing != key => {
            Err("this computer has another key already".to_owned())
        }
        Some(_) => Ok(key),
        None => {
            store.store(&key)?;
            Ok(key)
        }
    }
}

/// Whether the key is in memory, and when it goes.
#[derive(Debug, Clone)]
pub struct Vault {
    store: Arc<dyn KeyStore>,
    key: Option<Arc<NoteKey>>,
    /// `None`: never locks by itself.
    timeout: Option<Duration>,
    last_input: Option<Instant>,
    /// Input came since the last `tick`.
    activity: bool,
}

impl Default for Vault {
    fn default() -> Self {
        Self::with_store(Arc::new(KeyringKeyStore))
    }
}

impl Vault {
    pub fn with_store(store: Arc<dyn KeyStore>) -> Self {
        Self {
            store,
            key: None,
            timeout: Some(Duration::from_secs(
                u64::from(ncrs_core::notes_sync::NOTES_LOCK_DEFAULT_MINUTES) * 60,
            )),
            last_input: None,
            activity: false,
        }
    }

    pub fn store(&self) -> Arc<dyn KeyStore> {
        Arc::clone(&self.store)
    }

    pub fn is_unlocked(&self) -> bool {
        self.key.is_some()
    }

    pub fn key(&self) -> Option<Arc<NoteKey>> {
        self.key.clone()
    }

    /// 0 minutes: never.
    pub fn set_timeout_minutes(&mut self, minutes: u32) {
        self.timeout = (minutes > 0).then(|| Duration::from_secs(u64::from(minutes) * 60));
    }

    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    pub fn unlock(&mut self, key: NoteKey, now: Instant) {
        self.key = Some(Arc::new(key));
        self.last_input = Some(now);
        self.activity = false;
    }

    /// Drops the key from memory.
    pub fn lock(&mut self) {
        self.key = None;
        self.last_input = None;
        self.activity = false;
    }

    /// Something was done in the notes tab.
    pub fn note_activity(&mut self) {
        self.activity = self.key.is_some();
    }

    /// The clock moved to `now`. Locks when the timeout has passed without
    /// input since the last tick; answers whether it did.
    pub fn tick(&mut self, now: Instant) -> bool {
        if self.key.is_none() {
            return false;
        }
        if self.activity {
            self.activity = false;
            self.last_input = Some(now);
            return false;
        }
        let (Some(timeout), Some(last)) = (self.timeout, self.last_input) else {
            return false;
        };
        if now.saturating_duration_since(last) >= timeout {
            self.lock();
            return true;
        }
        false
    }
}
