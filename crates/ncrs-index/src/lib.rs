//! Encrypted workspace index: a SQLCipher database whose key lives in the
//! operating system's credential store.

mod documents;
mod error;
mod index;
mod journal;
mod key;
mod key_store;
mod locations;
mod migrations;
mod model;
mod names;
mod profile;
mod reorganisation;
mod review;
mod rule_suggestions;
mod rules;
#[cfg(test)]
mod test_support;

pub use error::{IndexError, KeyStoreError, RecoveryKeyError};
pub use index::Index;
pub use journal::{BatchKind, JournalItem, JournalState, NewJournalItem};
pub use key::RecoveryKey;
pub use key_store::{InMemoryKeyStore, KeyStore, KeyringKeyStore};
pub use model::{
    Amount, BatchId, ConflictReason, ContentHash, Currency, Document, DocumentFields, DocumentId,
    DocumentSource, DocumentStatus, Field, FileStamp, Finding, FindingKind, IndexStatistics,
    NewDocument, NoActionReason, PlanId, PlanItem, PlanOutcome, ReorganisationPlan, ReviewEntry,
    ReviewEntryId, ReviewReason, Rule, RuleEvidence, RuleId, RuleSpec, RuleSuggestion,
    SubtreePattern, TaxContext, TextMatch,
};
