//! Encrypted workspace index: a SQLCipher database whose key lives in the
//! operating system's credential store.

mod backup;
mod condition_store;
mod documents;
mod error;
#[cfg(test)]
mod eval_seed;
mod filed_inbox;
mod hermes_column;
mod hermes_counts;
mod index;
mod journal;
mod key;
mod key_store;
use ncrs_index_types::kinds;
mod locations;
mod migrations;
mod model;
mod names;
#[cfg(test)]
mod origin_migration_tests;
mod query_hit;
mod regex_limits;
mod review;
mod rule_suggestions;
mod rules;
mod saved_queries;
#[cfg(test)]
mod rule_target_tests;
mod search_index;
mod sender_aliases;
mod source_backlog;
mod tags;
#[cfg(test)]
mod test_support;

pub use backup::{migration_backups, set_aside_path};
pub use documents::{SNIPPET_MARK_END, SNIPPET_MARK_START};
pub use error::{IndexError, KeyStoreError, RecoveryKeyError};
pub use hermes_column::{FileTags, FolderCounts, IncludeOutcome, PathState};
pub use index::Index;
pub use journal::{
    BatchInfo, BatchKind, JournalItem, JournalState, NewJournalItem, SourceRoot, Transfer,
};
pub use key::RecoveryKey;
pub use key_store::{FileKeyStore, InMemoryKeyStore, KeyStore, KeyringKeyStore};
pub use kinds::{
    CustomKind, KindCatalog, KindEntry, KindLanguage, KindSkip, Kinds, MAX_FREE_KIND_CHARS,
};
pub use model::{amount_digits, amount_text, MINOR_UNITS_PER_UNIT};
pub use model::{iban_is_valid, is_iban_separator, is_tax_id_separator, tag_name};
pub use model::{
    Amount, BatchId, Combinator, Condition, ConditionKind, ContentHash, Currency, Document,
    DocumentFields, DocumentId, DocumentSource, DocumentStatus, Field, FieldOrigin, FieldOrigins,
    FileStamp, HermesCounts, IndexStatistics, NewDocument, QuerySpec, ReviewEntry, ReviewEntryId,
    ReviewReason, Rule, RuleConditions, RuleEvidence, RuleId, RuleOrigin, RuleSpec, RuleSuggestion,
    SavedQuery, SavedQueryId, SenderAlias, SenderAliasId, SenderAliasKind, SenderAliasState,
    SenderAliases, StatusCounts, SubtreePattern, TargetId,
};
pub use query_hit::QueryHit;
pub use regex_limits::compile_bounded_regex;
pub use search_index::SearchIndex;
pub use tags::{TagGroup, TagTally};
