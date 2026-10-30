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
mod regex_limits;
mod reorganisation;
mod review;
mod rule_suggestions;
mod rules;
mod saved_queries;
mod sender_aliases;
#[cfg(test)]
mod test_support;

pub use error::{IndexError, KeyStoreError, RecoveryKeyError};
pub use hermes_column::{FolderCounts, IncludeOutcome, PathState};
pub use index::Index;
pub use journal::{BatchKind, JournalItem, JournalState, NewJournalItem};
pub use key::RecoveryKey;
pub use key_store::{InMemoryKeyStore, KeyStore, KeyringKeyStore};
pub use model::{is_iban_separator, is_tax_id_separator};
pub use model::{
    Amount, BatchId, Combinator, Condition, ConditionKind, ConflictReason, ContentHash, Currency,
    Document, DocumentFields, DocumentId, DocumentSource, DocumentStatus, Field, FieldOrigin,
    FieldOrigins, FileStamp, Finding, FindingKind, HermesCounts, IndexStatistics, NewDocument,
    NoActionReason, PlanId, PlanItem, PlanOutcome, QuerySpec, ReorganisationPlan, ReviewEntry,
    ReviewEntryId, ReviewReason, Rule, RuleConditions, RuleEvidence, RuleId, RuleSpec,
    RuleSuggestion, SavedQuery, SavedQueryId, SenderAlias, SenderAliasId, SenderAliasKind,
    SenderAliasState, SenderAliases, StatusCounts, SubtreePattern, TaxContext,
};
pub use regex_limits::compile_bounded_regex;
