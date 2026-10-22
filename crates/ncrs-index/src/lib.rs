//! Encrypted workspace index: a SQLCipher database whose key lives in the
//! operating system's credential store.

mod error;
mod index;
mod key;
mod key_store;
mod migrations;

pub use error::{IndexError, KeyStoreError, RecoveryKeyError};
pub use index::Index;
pub use key::RecoveryKey;
pub use key_store::{InMemoryKeyStore, KeyStore, KeyringKeyStore};
