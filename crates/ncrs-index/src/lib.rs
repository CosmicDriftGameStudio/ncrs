//! Encrypted workspace index: a SQLCipher database whose key lives in the
//! operating system's credential store.

mod documents;
mod error;
mod hermes_column;
mod hermes_counts;
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
mod saved_queries;
#[cfg(test)]
mod test_support;

pub use error::{IndexError, KeyStoreError, RecoveryKeyError};
pub use hermes_column::{FolderCounts, IncludeOutcome, PathState};
pub use index::Index;
pub use journal::{BatchKind, JournalItem, JournalState, NewJournalItem};
pub use key::RecoveryKey;
pub use key_store::{InMemoryKeyStore, KeyStore, KeyringKeyStore};
pub use model::{
    Amount, BatchId, ConflictReason, ContentHash, Currency, Document, DocumentFields, DocumentId,
    DocumentSource, DocumentStatus, Field, FieldOrigin, FieldOrigins, FileStamp, Finding,
    FindingKind, HermesCounts, IndexStatistics, NewDocument, NoActionReason, PlanId, PlanItem,
    PlanOutcome, QuerySpec, ReorganisationPlan, ReviewEntry, ReviewEntryId, ReviewReason, Rule,
    RuleEvidence, RuleId, RuleSpec, RuleSuggestion, SavedQuery, SavedQueryId, StatusCounts,
    SubtreePattern, TaxContext, TextMatch,
};
