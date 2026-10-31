use std::fmt;
use std::path::PathBuf;

use crate::model::{DocumentId, ReviewEntryId, RuleId, SavedQueryId, SenderAliasId, TaxContextId};

/// Failures of the credential store, without ever carrying the secret.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyStoreError {
    /// No credential store could be reached (for example Linux without a
    /// Secret Service daemon). There is deliberately no file fallback.
    Unavailable(String),
    /// `store_new` found an entry and refused to replace it.
    AlreadyExists,
}

impl fmt::Display for KeyStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(reason) => write!(f, "credential store unavailable: {reason}"),
            Self::AlreadyExists => write!(f, "a key is already stored; refusing to overwrite it"),
        }
    }
}

impl std::error::Error for KeyStoreError {}

#[derive(Debug)]
#[non_exhaustive]
pub enum IndexError {
    /// `open` or `restore_key` was pointed at a file that is not there; the
    /// database is never created implicitly.
    NotFound,
    /// `create_*` was pointed at a file that is already there.
    AlreadyExists,
    /// `restore_key` found an entry in the credential store and left it alone.
    KeyStoreNotEmpty,
    /// The database file exists but the credential store has no key for it.
    /// A new key would make the data unreadable for good, so none is made.
    KeyMissing,
    /// The stored key does not open the database.
    WrongKey,
    /// `replace_key` was pointed at a plain database, which has no key.
    NotEncrypted,
    /// `encrypt_plain` was pointed at a database that is already encrypted.
    NotPlain,
    /// The stored entry is not a 32 byte key.
    InvalidKeyLength {
        found: usize,
    },
    /// The key read back after `store_new` differs from the one written.
    KeyStoreVerificationFailed,
    /// The database was written by a newer version with more migrations.
    SchemaTooNew {
        found: u32,
        supported: u32,
    },
    /// A schema step failed, or left rows that violate a foreign key. Nothing of the update was kept: the file is at
    /// the version `from` it was found at, with its old content.
    MigrationFailed {
        from: u32,
        step: u32,
        reason: String,
    },
    /// A value read from the database is not one the code writes.
    CorruptValue {
        column: &'static str,
        value: String,
    },
    /// The path is not UTF-8. It is never stored lossily, because moves and
    /// filing rely on the stored path being the real one.
    NonUtf8Path(PathBuf),
    EmptyName {
        what: &'static str,
    },
    RuleWithoutCondition,
    /// The regex condition at `position` of the condition list does not compile
    /// within the size limits.
    InvalidRegex {
        position: usize,
        reason: String,
    },
    /// A document with this content hash is already indexed.
    DuplicateHash(DocumentId),
    DocumentNotFound(DocumentId),
    RuleNotFound(RuleId),
    SavedQueryNotFound(SavedQueryId),
    SenderAliasNotFound(SenderAliasId),
    /// Another sender already holds this value as an offer or confirmed alias.
    SenderAliasTaken {
        sender: String,
    },
    /// No stored tax context has this short name.
    UnknownTaxContext(String),
    /// There is no stored tax context with this id.
    TaxContextNotFound(TaxContextId),
    /// A tax context was refused; nothing was changed.
    InvalidTaxContext(TaxContextProblem),
    /// Another saved query already has this name.
    SavedQueryNameTaken(String),
    /// The document already has an unresolved review entry.
    ReviewAlreadyOpen(ReviewEntryId),
    /// Unknown, or already resolved.
    ReviewNotFound(ReviewEntryId),
    KeyStore(KeyStoreError),
    RecoveryKey(RecoveryKeyError),
    Random(String),
    Io(std::io::Error),
    Sqlite(rusqlite::Error),
}

impl fmt::Display for IndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(f, "the index database does not exist"),
            Self::AlreadyExists => write!(f, "the index database already exists"),
            Self::KeyStoreNotEmpty => {
                write!(
                    f,
                    "the credential store already holds a key; refusing to replace it"
                )
            }
            Self::KeyMissing => write!(f, "the index database exists but its key is missing"),
            Self::WrongKey => write!(f, "the stored key does not open the index database"),
            Self::NotEncrypted => write!(
                f,
                "the index database is not encrypted, so it has no key to replace"
            ),
            Self::NotPlain => write!(f, "the index database is already encrypted"),
            Self::InvalidKeyLength { found } => {
                write!(f, "the stored index key is {found} bytes, expected 32")
            }
            Self::KeyStoreVerificationFailed => {
                write!(
                    f,
                    "the credential store returned a different key than was written"
                )
            }
            Self::SchemaTooNew { found, supported } => write!(
                f,
                "the index schema is version {found}, this build supports up to {supported}"
            ),
            Self::MigrationFailed { from, step, reason } => write!(
                f,
                "updating the index from schema version {from} failed at step {step}: {reason}; \
                 the index file was left unchanged"
            ),
            Self::CorruptValue { column, value } => {
                write!(f, "the index holds an unknown value '{value}' in {column}")
            }
            Self::NonUtf8Path(path) => write!(f, "the path {} is not valid UTF-8", path.display()),
            Self::EmptyName { what } => write!(f, "a {what} name must not be empty"),
            Self::RuleWithoutCondition => {
                write!(f, "a rule needs a text match or a sender to match")
            }
            Self::InvalidRegex { position, reason } => {
                write!(f, "condition {position}: invalid regex: {reason}")
            }
            Self::DuplicateHash(id) => {
                write!(f, "a document with this content is already indexed ({id})")
            }
            Self::DocumentNotFound(id) => write!(f, "there is no document {id}"),
            Self::RuleNotFound(id) => write!(f, "there is no rule {id}"),
            Self::SavedQueryNotFound(id) => write!(f, "there is no saved query {id}"),
            Self::UnknownTaxContext(short_name) => {
                write!(
                    f,
                    "there is no tax context with the short name '{short_name}'"
                )
            }
            Self::TaxContextNotFound(id) => write!(f, "there is no tax context {}", id.get()),
            Self::InvalidTaxContext(problem) => problem.fmt(f),
            Self::SenderAliasNotFound(id) => write!(f, "there is no sender alias {id}"),
            Self::SenderAliasTaken { sender } => {
                write!(f, "this value is already an alias of {sender}")
            }
            Self::SavedQueryNameTaken(name) => {
                write!(f, "a saved query named '{name}' already exists")
            }
            Self::ReviewAlreadyOpen(id) => {
                write!(f, "the document already has an open review entry ({id})")
            }
            Self::ReviewNotFound(id) => {
                write!(f, "there is no open review entry {id}")
            }
            Self::KeyStore(err) => err.fmt(f),
            Self::RecoveryKey(err) => err.fmt(f),
            Self::Random(reason) => write!(f, "no random bytes available: {reason}"),
            Self::Io(err) => err.fmt(f),
            Self::Sqlite(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for IndexError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::KeyStore(err) => Some(err),
            Self::Io(err) => Some(err),
            Self::Sqlite(err) => Some(err),
            _ => None,
        }
    }
}

impl From<KeyStoreError> for IndexError {
    fn from(err: KeyStoreError) -> Self {
        Self::KeyStore(err)
    }
}

impl From<rusqlite::Error> for IndexError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Sqlite(err)
    }
}

/// Why a tax context name or short name, or a change to one, was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TaxContextProblem {
    NameEmpty,
    NameTooLong {
        max: usize,
    },
    NameHasControlCharacter,
    NameTaken,
    ShortNameEmpty,
    ShortNameTooLong {
        max: usize,
    },
    ShortNameForbiddenCharacter(char),
    ShortNameStartsWithDot,
    ShortNameHasDoubleDot,
    ShortNameReserved,
    ShortNameTaken,
    /// "No context" is built in and cannot be renamed or deleted.
    NoContextIsFixed,
    /// Documents cannot move to the context that is being deleted.
    MoveToItself,
}

impl fmt::Display for TaxContextProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NameEmpty => write!(f, "the name must not be empty"),
            Self::NameTooLong { max } => write!(f, "the name is longer than {max} characters"),
            Self::NameHasControlCharacter => write!(f, "the name contains a control character"),
            Self::NameTaken => write!(f, "another tax context already has this name"),
            Self::ShortNameEmpty => write!(f, "the short name must not be empty"),
            Self::ShortNameTooLong { max } => {
                write!(f, "the short name is longer than {max} characters")
            }
            Self::ShortNameForbiddenCharacter(ch) => {
                write!(f, "'{ch}' is not allowed in a short name")
            }
            Self::ShortNameStartsWithDot => write!(f, "the short name must not start with '.'"),
            Self::ShortNameHasDoubleDot => write!(f, "the short name must not contain '..'"),
            Self::ShortNameReserved => write!(f, "this short name is reserved"),
            Self::ShortNameTaken => write!(f, "another tax context already has this short name"),
            Self::NoContextIsFixed => write!(f, "'no context' cannot be changed"),
            Self::MoveToItself => write!(f, "documents cannot move to the context being deleted"),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RecoveryKeyError {
    /// Not 52 base32 characters once spaces and hyphens are removed.
    WrongLength {
        found: usize,
    },
    InvalidCharacter(char),
    /// The last character carries bits that a 32 byte key never sets.
    NonCanonical,
}

impl fmt::Display for RecoveryKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongLength { found } => {
                write!(f, "a recovery key has 52 characters, this has {found}")
            }
            Self::InvalidCharacter(ch) => write!(f, "'{ch}' is not part of a recovery key"),
            Self::NonCanonical => write!(f, "this is not a valid recovery key"),
        }
    }
}

impl std::error::Error for RecoveryKeyError {}
