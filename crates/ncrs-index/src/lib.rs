//! Encrypted workspace index: a SQLCipher database whose key lives in the
//! operating system's credential store.

mod documents;
mod error;
mod index;
mod key;
mod key_store;
mod migrations;
mod model;
mod names;
mod review;
mod rules;
#[cfg(test)]
mod test_support;

pub use error::{IndexError, KeyStoreError, RecoveryKeyError};
pub use index::Index;
pub use key::RecoveryKey;
pub use key_store::{InMemoryKeyStore, KeyStore, KeyringKeyStore};
pub use model::{
    Amount, ContentHash, Currency, Document, DocumentFields, DocumentId, DocumentSource,
    DocumentStatus, Field, NewDocument, ReviewEntry, ReviewEntryId, ReviewReason, Rule, RuleId,
    RuleSpec, TaxContext, TextMatch,
};
