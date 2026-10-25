use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};
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
id_newtype!(PlanId);
id_newtype!(BatchId);
id_newtype!(SavedQueryId);

/// SHA-256 of the file content. The index stores it; hashing is not its job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContentHash(pub [u8; 32]);

pub fn corrupt(column: &'static str, value: &str) -> IndexError {
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
    /// The rules found a place for it; the filing job has not moved it yet.
    Planned,
    InReview,
    Filed,
}

impl DocumentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Planned => "planned",
            Self::InReview => "in_review",
            Self::Filed => "filed",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, IndexError> {
        match value {
            "new" => Ok(Self::New),
            "planned" => Ok(Self::Planned),
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
    /// The place the rules chose is taken by another file.
    TargetExists,
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
            Self::TargetExists => ("target_exists", None),
            Self::MissingField(field) => ("missing_field", Some(field)),
            Self::LowConfidence(field) => ("low_confidence", Some(field)),
        }
    }

    pub(crate) fn parse(reason: &str, field_name: Option<&str>) -> Result<Self, IndexError> {
        let named = field_name.map(Field::parse).transpose()?;
        match (reason, named) {
            ("no_matching_rule", None) => Ok(Self::NoMatchingRule),
            ("unconfirmed_rule", None) => Ok(Self::UnconfirmedRule),
            ("target_exists", None) => Ok(Self::TargetExists),
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

/// What a saved query asks for; a criterion left out matches everything.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuerySpec {
    /// The year of the document date.
    pub year: Option<i32>,
    pub tax_context: Option<TaxContext>,
    /// Compared without regard to case.
    pub kind: Option<String>,
    /// A document must carry all of them.
    pub tags: BTreeSet<String>,
    /// Words that all occur in the full text.
    pub full_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedQuery {
    pub id: SavedQueryId,
    pub name: String,
    pub spec: QuerySpec,
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
    /// The user discarded this condition and target before; nothing was
    /// stored.
    Discarded,
}

/// What looks wrong about a file of an archive location. The order of the
/// variants is not a priority; the planner owns that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FindingKind {
    Duplicate,
    Misplaced,
    YearMismatch,
    NameOutlier,
}

impl FindingKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Duplicate => "duplicate",
            Self::Misplaced => "misplaced",
            Self::YearMismatch => "year_mismatch",
            Self::NameOutlier => "name_outlier",
        }
    }

    fn parse(value: &str) -> Result<Self, IndexError> {
        match value {
            "duplicate" => Ok(Self::Duplicate),
            "misplaced" => Ok(Self::Misplaced),
            "year_mismatch" => Ok(Self::YearMismatch),
            "name_outlier" => Ok(Self::NameOutlier),
            other => Err(corrupt("reorganisation_finding.kind", other)),
        }
    }
}

/// A finding with the reason it was made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// Another file with the same content stays; `keeper` is its path below
    /// the location.
    Duplicate { keeper: PathBuf },
    /// A confirmed rule, or a suggestion that meets the evidence thresholds,
    /// files documents like this one elsewhere.
    Misplaced { rule: RuleId },
    /// The year folder of the path is not the year of the document's date.
    YearMismatch { folder_year: i32, date_year: i32 },
    /// The file name does not follow the pattern of its folder; `pattern` is
    /// the name pattern the folder follows.
    NameOutlier { pattern: String },
}

impl Finding {
    pub fn kind(&self) -> FindingKind {
        match self {
            Self::Duplicate { .. } => FindingKind::Duplicate,
            Self::Misplaced { .. } => FindingKind::Misplaced,
            Self::YearMismatch { .. } => FindingKind::YearMismatch,
            Self::NameOutlier { .. } => FindingKind::NameOutlier,
        }
    }

    pub(crate) fn detail_column(&self) -> Result<String, IndexError> {
        Ok(match self {
            Self::Duplicate { keeper } => keeper
                .to_str()
                .ok_or_else(|| IndexError::NonUtf8Path(keeper.clone()))?
                .to_owned(),
            Self::Misplaced { rule } => rule.get().to_string(),
            Self::YearMismatch {
                folder_year,
                date_year,
            } => format!("{folder_year}:{date_year}"),
            Self::NameOutlier { pattern } => pattern.clone(),
        })
    }

    pub(crate) fn from_columns(kind: &str, detail: &str) -> Result<Self, IndexError> {
        let bad = || corrupt("reorganisation_finding.detail", detail);
        Ok(match FindingKind::parse(kind)? {
            FindingKind::Duplicate => Self::Duplicate {
                keeper: PathBuf::from(detail),
            },
            FindingKind::Misplaced => Self::Misplaced {
                rule: RuleId::new(detail.parse().map_err(|_unparsable| bad())?),
            },
            FindingKind::YearMismatch => {
                let (folder, date) = detail.split_once(':').ok_or_else(bad)?;
                Self::YearMismatch {
                    folder_year: folder.parse().map_err(|_unparsable| bad())?,
                    date_year: date.parse().map_err(|_unparsable| bad())?,
                }
            }
            FindingKind::NameOutlier => Self::NameOutlier {
                pattern: detail.to_owned(),
            },
        })
    }
}

/// Why a proposed target is not an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictReason {
    /// Something is at the target on disk.
    TargetExists,
    /// Another item of the same plan wants the same target.
    TargetTakenByOtherItem,
    /// The target would not stay below the location.
    TargetLeavesLocation,
}

impl ConflictReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::TargetExists => "target_exists",
            Self::TargetTakenByOtherItem => "target_taken_by_other_item",
            Self::TargetLeavesLocation => "target_leaves_location",
        }
    }

    fn parse(value: &str) -> Result<Self, IndexError> {
        match value {
            "target_exists" => Ok(Self::TargetExists),
            "target_taken_by_other_item" => Ok(Self::TargetTakenByOtherItem),
            "target_leaves_location" => Ok(Self::TargetLeavesLocation),
            other => Err(corrupt("reorganisation_item.detail", other)),
        }
    }
}

/// Why a finding has no proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoActionReason {
    /// The fields the folder's pattern needs for a name are not known.
    MissingFields,
}

impl NoActionReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::MissingFields => "missing_fields",
        }
    }

    fn parse(value: &str) -> Result<Self, IndexError> {
        match value {
            "missing_fields" => Ok(Self::MissingFields),
            other => Err(corrupt("reorganisation_item.detail", other)),
        }
    }
}

/// What the plan proposes for one file. There is no variant that deletes.
/// Targets are paths below the location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanOutcome {
    Move {
        target: PathBuf,
    },
    /// A move within the file's folder.
    Rename {
        target: PathBuf,
    },
    /// `target` is `None` when it would have left the location.
    Conflict {
        target: Option<PathBuf>,
        reason: ConflictReason,
    },
    NoAction(NoActionReason),
}

impl PlanOutcome {
    pub(crate) fn to_columns(
        &self,
    ) -> Result<(&'static str, Option<&str>, Option<&'static str>), IndexError> {
        fn text(path: &Path) -> Result<&str, IndexError> {
            path.to_str()
                .ok_or_else(|| IndexError::NonUtf8Path(path.to_path_buf()))
        }
        Ok(match self {
            Self::Move { target } => ("move", Some(text(target)?), None),
            Self::Rename { target } => ("rename", Some(text(target)?), None),
            Self::Conflict { target, reason } => (
                "conflict",
                target.as_deref().map(text).transpose()?,
                Some(reason.as_str()),
            ),
            Self::NoAction(reason) => ("no_action", None, Some(reason.as_str())),
        })
    }

    pub(crate) fn from_columns(
        outcome: &str,
        target: Option<String>,
        detail: Option<&str>,
    ) -> Result<Self, IndexError> {
        let missing = |column: &'static str| corrupt(column, outcome);
        Ok(match outcome {
            "move" => Self::Move {
                target: PathBuf::from(target.ok_or_else(|| missing("reorganisation_item.target"))?),
            },
            "rename" => Self::Rename {
                target: PathBuf::from(target.ok_or_else(|| missing("reorganisation_item.target"))?),
            },
            "conflict" => Self::Conflict {
                target: target.map(PathBuf::from),
                reason: ConflictReason::parse(
                    detail.ok_or_else(|| missing("reorganisation_item.detail"))?,
                )?,
            },
            "no_action" => Self::NoAction(NoActionReason::parse(
                detail.ok_or_else(|| missing("reorganisation_item.detail"))?,
            )?),
            other => return Err(corrupt("reorganisation_item.outcome", other)),
        })
    }
}

/// One file of the plan, as it was when the plan was made: `hash` and
/// `source` are what execution checks against the file on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanItem {
    /// Below the location.
    pub source: PathBuf,
    pub hash: ContentHash,
    /// Every finding that applies; the outcome follows the one that wins.
    pub findings: Vec<Finding>,
    pub outcome: PlanOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReorganisationPlan {
    pub id: PlanId,
    pub location: PathBuf,
    pub created_at: DateTime<Utc>,
    /// Sorted by source.
    pub items: Vec<PlanItem>,
}
