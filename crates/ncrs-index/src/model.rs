use std::collections::BTreeSet;
use std::fmt;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, NaiveDate, Utc};

use crate::error::IndexError;

macro_rules! id_newtype {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(i64);

        impl $name {
            pub(crate) fn new(raw: i64) -> Self {
                Self(raw)
            }

            pub(crate) fn get(self) -> i64 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

id_newtype!(DocumentId);
id_newtype!(RuleId);
id_newtype!(ReviewEntryId);

/// SHA-256 of the file content. The index stores it; hashing is not its job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContentHash(pub [u8; 32]);

fn corrupt(column: &'static str, value: &str) -> IndexError {
    IndexError::CorruptValue {
        column,
        value: value.to_owned(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DocumentSource {
    Inbox,
    /// Indexed where it lies; the scan never moves or changes it.
    Archive,
}

impl DocumentSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Archive => "archive",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, IndexError> {
        match value {
            "inbox" => Ok(Self::Inbox),
            "archive" => Ok(Self::Archive),
            other => Err(corrupt("document.source", other)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DocumentStatus {
    New,
    InReview,
    Filed,
}

impl DocumentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::InReview => "in_review",
            Self::Filed => "filed",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, IndexError> {
        match value {
            "new" => Ok(Self::New),
            "in_review" => Ok(Self::InReview),
            "filed" => Ok(Self::Filed),
            other => Err(corrupt("document.status", other)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaxContext {
    Company,
    Private,
    None,
}

impl TaxContext {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Company => "company",
            Self::Private => "private",
            Self::None => "none",
        }
    }

    pub(crate) fn parse(column: &'static str, value: &str) -> Result<Self, IndexError> {
        match value {
            "company" => Ok(Self::Company),
            "private" => Ok(Self::Private),
            "none" => Ok(Self::None),
            other => Err(corrupt(column, other)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    Sender,
    Kind,
    TaxContext,
    Date,
    Number,
    Amount,
}

impl Field {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sender => "sender",
            Self::Kind => "kind",
            Self::TaxContext => "tax_context",
            Self::Date => "date",
            Self::Number => "number",
            Self::Amount => "amount",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, IndexError> {
        match value {
            "sender" => Ok(Self::Sender),
            "kind" => Ok(Self::Kind),
            "tax_context" => Ok(Self::TaxContext),
            "date" => Ok(Self::Date),
            "number" => Ok(Self::Number),
            "amount" => Ok(Self::Amount),
            other => Err(corrupt("review_entry.field", other)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReviewReason {
    NoMatchingRule,
    UnconfirmedRule,
    MissingField(Field),
    LowConfidence(Field),
}

impl ReviewReason {
    /// The `reason` and `field` columns; `field` is set exactly for the
    /// reasons that name one.
    pub(crate) fn as_columns(self) -> (&'static str, Option<Field>) {
        match self {
            Self::NoMatchingRule => ("no_matching_rule", None),
            Self::UnconfirmedRule => ("unconfirmed_rule", None),
            Self::MissingField(field) => ("missing_field", Some(field)),
            Self::LowConfidence(field) => ("low_confidence", Some(field)),
        }
    }

    pub(crate) fn parse(reason: &str, field_name: Option<&str>) -> Result<Self, IndexError> {
        let named = field_name.map(Field::parse).transpose()?;
        match (reason, named) {
            ("no_matching_rule", None) => Ok(Self::NoMatchingRule),
            ("unconfirmed_rule", None) => Ok(Self::UnconfirmedRule),
            ("missing_field", Some(of)) => Ok(Self::MissingField(of)),
            ("low_confidence", Some(of)) => Ok(Self::LowConfidence(of)),
            _ => Err(corrupt("review_entry.reason", reason)),
        }
    }
}

/// Three ASCII upper-case letters.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Currency(String);

impl Currency {
    pub fn new(code: &str) -> Option<Self> {
        (code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_uppercase()))
            .then(|| Self(code.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// `minor_units` is the smallest unit of the currency (cents for EUR).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Amount {
    pub minor_units: i64,
    pub currency: Currency,
}

/// Size and modification time of a file when it was read. Equal stamps let a
/// scan skip hashing an unchanged file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStamp {
    pub size: u64,
    pub mtime: SystemTime,
}

impl FileStamp {
    /// Nanoseconds since the epoch, negative before it.
    pub(crate) fn mtime_nanos(self) -> i64 {
        match self.mtime.duration_since(UNIX_EPOCH) {
            Ok(after) => i64::try_from(after.as_nanos()).unwrap_or(i64::MAX),
            Err(before) => -i64::try_from(before.duration().as_nanos()).unwrap_or(i64::MAX),
        }
    }

    pub(crate) fn size_column(self) -> i64 {
        i64::try_from(self.size).unwrap_or(i64::MAX)
    }

    pub(crate) fn from_columns(
        size_column: Option<i64>,
        mtime_column: Option<i64>,
    ) -> Result<Option<Self>, IndexError> {
        let (Some(stored_size), Some(nanos)) = (size_column, mtime_column) else {
            return Ok(None);
        };
        let offset = Duration::from_nanos(nanos.unsigned_abs());
        let mtime = if nanos >= 0 {
            UNIX_EPOCH.checked_add(offset)
        } else {
            UNIX_EPOCH.checked_sub(offset)
        }
        .ok_or_else(|| corrupt("document.file_mtime", &nanos.to_string()))?;
        let size = u64::try_from(stored_size)
            .map_err(|_negative| corrupt("document.file_size", &stored_size.to_string()))?;
        Ok(Some(Self { size, mtime }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDocument {
    pub hash: ContentHash,
    pub path: PathBuf,
    pub source: DocumentSource,
    pub received_at: DateTime<Utc>,
    pub stamp: Option<FileStamp>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentFields {
    pub sender: Option<String>,
    pub kind: Option<String>,
    pub tax_context: Option<TaxContext>,
    pub date: Option<NaiveDate>,
    pub number: Option<String>,
    pub amount: Option<Amount>,
    pub tags: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub id: DocumentId,
    pub hash: ContentHash,
    pub path: PathBuf,
    pub source: DocumentSource,
    pub received_at: DateTime<Utc>,
    pub status: DocumentStatus,
    pub fields: DocumentFields,
    pub stamp: Option<FileStamp>,
    /// Set when the file is gone or no longer has this content. A missing
    /// document is not found by path; its hash still is.
    pub missing_since: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextMatch {
    Contains(String),
    Regex(String),
}

/// The extract patterns are regex sources the index only stores.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleSpec {
    pub match_text: Option<TextMatch>,
    pub match_sender: Option<String>,
    pub extract_number: Option<String>,
    pub extract_date: Option<String>,
    pub extract_amount: Option<String>,
    pub kind: String,
    pub tax_context: TaxContext,
    pub tags: BTreeSet<String>,
    pub filing_pattern: String,
    pub confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub id: RuleId,
    pub spec: RuleSpec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexStatistics {
    pub schema_version: u32,
    pub document_count: u64,
    pub open_reviews: u64,
    pub full_text_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewEntry {
    pub id: ReviewEntryId,
    pub document_id: DocumentId,
    pub reason: ReviewReason,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

pub fn timestamp_from_column(
    column: &'static str,
    seconds: i64,
) -> Result<DateTime<Utc>, IndexError> {
    DateTime::from_timestamp(seconds, 0).ok_or_else(|| corrupt(column, &seconds.to_string()))
}

pub fn date_from_column(value: &str) -> Result<NaiveDate, IndexError> {
    value
        .parse()
        .map_err(|_unparsable| corrupt("document.document_date", value))
}

pub fn currency_from_column(value: &str) -> Result<Currency, IndexError> {
    Currency::new(value).ok_or_else(|| corrupt("document.currency", value))
}

pub fn hash_from_column(bytes: &[u8]) -> Result<ContentHash, IndexError> {
    <[u8; 32]>::try_from(bytes)
        .map(ContentHash)
        .map_err(|_wrong_length| corrupt("document.hash", &format!("{} bytes", bytes.len())))
}

/// What was learned about one subtree of an archive location. A subtree is
/// named by its first constant folder (empty for files outside any).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtreePattern {
    pub subtree: String,
    /// A filing pattern in the syntax of the rules; `None` when the documents
    /// of the subtree follow no common structure.
    pub pattern: Option<String>,
    /// How many documents follow `pattern`.
    pub documents: u32,
    /// How many documents the subtree has.
    pub total: u32,
}

/// Why a suggested rule was suggested: the phrase was in `matching_in_folder`
/// of the `folder_documents` documents of its folder and in
/// `matching_elsewhere` documents outside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleEvidence {
    pub matching_in_folder: u32,
    pub folder_documents: u32,
    pub matching_elsewhere: u32,
}

/// What storing a suggested rule did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleSuggestion {
    Created(RuleId),
    /// An unconfirmed rule with the same condition and target had its
    /// evidence replaced.
    Refreshed(RuleId),
    /// The user already confirmed a rule with this condition and target; it
    /// was left as it is.
    AlreadyConfirmed(RuleId),
}
