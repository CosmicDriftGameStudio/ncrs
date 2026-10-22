use std::fmt;

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
