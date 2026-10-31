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
id_newtype!(SenderAliasId);

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
pub enum Field {
    Sender,
    Kind,
    Date,
    Number,
    Amount,
}

impl Field {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sender => "sender",
            Self::Kind => "kind",
            Self::Date => "date",
            Self::Number => "number",
            Self::Amount => "amount",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, IndexError> {
        match value {
            "sender" => Ok(Self::Sender),
            "kind" => Ok(Self::Kind),
            "date" => Ok(Self::Date),
            "number" => Ok(Self::Number),
            "amount" => Ok(Self::Amount),
            other => Err(corrupt("review_entry.field", other)),
        }
    }
}

impl Field {
    pub const ALL: [Self; 5] = [
        Self::Sender,
        Self::Kind,
        Self::Date,
        Self::Number,
        Self::Amount,
    ];
}

/// Where the stored value of a document field came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldOrigin {
    Unknown,
    /// `None` when the rule has been deleted since, or the rules step found the
    /// value without a rule.
    Rule(Option<RuleId>),
    Ai {
        confidence_percent: u8,
    },
    Manual,
}

impl FieldOrigin {
    pub(crate) fn kind_column(&self) -> Option<&'static str> {
        match self {
            Self::Unknown => None,
            Self::Rule(_) => Some("rule"),
            Self::Ai { .. } => Some("ai"),
            Self::Manual => Some("manual"),
        }
    }

    pub(crate) fn rule_column(&self) -> Option<i64> {
        match self {
            Self::Rule(rule) => rule.map(RuleId::get),
            _ => None,
        }
    }

    pub(crate) fn confidence_column(&self) -> Option<i64> {
        match self {
            Self::Ai { confidence_percent } => Some(i64::from(*confidence_percent)),
            _ => None,
        }
    }

    pub(crate) fn from_columns(
        origin: &str,
        rule_id: Option<i64>,
        confidence: Option<i64>,
    ) -> Result<Self, IndexError> {
        match origin {
            "rule" => Ok(Self::Rule(rule_id.map(RuleId::new))),
            "ai" => {
                let stored = confidence
                    .ok_or_else(|| corrupt("document_field_origin.confidence", "NULL"))?;
                let confidence_percent = u8::try_from(stored)
                    .ok()
                    .filter(|percent| *percent <= 100)
                    .ok_or_else(|| {
                        corrupt("document_field_origin.confidence", &stored.to_string())
                    })?;
                Ok(Self::Ai { confidence_percent })
            }
            "manual" => Ok(Self::Manual),
            other => Err(corrupt("document_field_origin.origin", other)),
        }
    }
}

/// The origin of each field of one document; a field without a stored origin
/// is `Unknown`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FieldOrigins(std::collections::HashMap<Field, FieldOrigin>);

impl FieldOrigins {
    pub(crate) fn new(stored: std::collections::HashMap<Field, FieldOrigin>) -> Self {
        Self(stored)
    }

    pub fn of(&self, field: Field) -> FieldOrigin {
        self.0.get(&field).cloned().unwrap_or(FieldOrigin::Unknown)
    }

    /// These origins, with `field` coming from `origin`.
    pub fn with(mut self, field: Field, origin: FieldOrigin) -> Self {
        self.0.insert(field, origin);
        self
    }

    /// The fields the user set by hand, in the order of `Field::ALL`.
    pub fn manual_fields(&self) -> Vec<Field> {
        Field::ALL
            .into_iter()
            .filter(|field| self.0.get(field) == Some(&FieldOrigin::Manual))
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReviewReason {
    NoMatchingRule,
    UnconfirmedRule,
    /// Several confirmed rules match and do not agree on the result.
    RuleConflict,
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
            Self::RuleConflict => ("rule_conflict", None),
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
            ("rule_conflict", None) => Ok(Self::RuleConflict),
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
    /// Compared without regard to case.
    pub sender: Option<String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SenderAliasKind {
    Name,
    Domain,
    Iban,
    TaxId,
}

impl SenderAliasKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Domain => "domain",
            Self::Iban => "iban",
            Self::TaxId => "tax_id",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, IndexError> {
        match value {
            "name" => Ok(Self::Name),
            "domain" => Ok(Self::Domain),
            "iban" => Ok(Self::Iban),
            "tax_id" => Ok(Self::TaxId),
            other => Err(corrupt("sender_alias.kind", other)),
        }
    }

    /// The form in which values of this kind are stored and compared.
    pub fn normalise(self, value: &str) -> String {
        match self {
            Self::Name => value
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase(),
            Self::Domain => {
                let lowered = value.trim().to_lowercase();
                lowered.strip_prefix("www.").unwrap_or(&lowered).to_owned()
            }
            Self::Iban => value
                .chars()
                .filter(|ch| !is_iban_separator(*ch))
                .flat_map(char::to_uppercase)
                .collect(),
            Self::TaxId => value
                .chars()
                .filter(|ch| !is_tax_id_separator(*ch))
                .flat_map(char::to_uppercase)
                .collect(),
        }
    }
}

pub fn is_iban_separator(ch: char) -> bool {
    ch.is_whitespace()
}

/// ISO 13616: separators are ignored, case does not matter.
pub fn iban_is_valid(value: &str) -> bool {
    let chars: Vec<char> = value
        .chars()
        .filter(|ch| !is_iban_separator(*ch))
        .map(|ch| ch.to_ascii_uppercase())
        .collect();
    if !(15..=34).contains(&chars.len()) || !chars.iter().all(char::is_ascii_alphanumeric) {
        return false;
    }
    let country_is_letters = chars.iter().take(2).all(char::is_ascii_alphabetic);
    let check_is_digits = chars.iter().skip(2).take(2).all(char::is_ascii_digit);
    if !country_is_letters || !check_is_digits {
        return false;
    }
    let remainder =
        chars
            .iter()
            .skip(4)
            .chain(chars.iter().take(4))
            .try_fold(0, |rest: u32, ch| {
                let digit_value = ch.to_digit(36)?;
                let shift = if digit_value < 10 { 10 } else { 100 };
                Some((rest * shift + digit_value) % 97)
            });
    remainder == Some(1)
}

pub fn is_tax_id_separator(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '/' | '-' | '.')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SenderAliasState {
    Offered,
    Confirmed,
    Discarded,
}

impl SenderAliasState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Offered => "offered",
            Self::Confirmed => "confirmed",
            Self::Discarded => "discarded",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, IndexError> {
        match value {
            "offered" => Ok(Self::Offered),
            "confirmed" => Ok(Self::Confirmed),
            "discarded" => Ok(Self::Discarded),
            other => Err(corrupt("sender_alias.state", other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SenderAlias {
    pub id: SenderAliasId,
    pub sender: String,
    pub kind: SenderAliasKind,
    /// Normalised.
    pub value: String,
    pub state: SenderAliasState,
}

/// The confirmed aliases, which is what planning and detection need.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SenderAliases {
    senders: std::collections::HashMap<(SenderAliasKind, String), String>,
}

impl SenderAliases {
    /// Entries are (kind, value, sender); values are normalised here.
    pub fn from_confirmed<'a>(
        entries: impl IntoIterator<Item = (SenderAliasKind, &'a str, &'a str)>,
    ) -> Self {
        Self {
            senders: entries
                .into_iter()
                .map(|(kind, value, sender)| ((kind, kind.normalise(value)), sender.to_owned()))
                .collect(),
        }
    }

    /// The sender a confirmed alias of this kind and value stands for.
    pub fn sender_for(&self, kind: SenderAliasKind, raw_value: &str) -> Option<&str> {
        self.senders
            .get(&(kind, kind.normalise(raw_value)))
            .map(String::as_str)
    }

    /// The sender a confirmed name alias maps `name` to, else `name` itself.
    pub fn canonical_sender<'a>(&'a self, name: &'a str) -> &'a str {
        self.sender_for(SenderAliasKind::Name, name).unwrap_or(name)
    }
}

/// Substring: the text contains the value, without regard to case. Word: the
/// text contains the word or phrase between word boundaries. Regex: the value
/// is a regex source run over the text. Sender: the sender of the document
/// equals the value, without regard to case. SenderAddress: an e-mail domain or
/// address found in the text. Iban and TaxId: the number occurs in the text,
/// whatever the spacing. Recipient: the name or address stands in the addressee
/// block. Folder: the folder the document lies in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConditionKind {
    Substring,
    Word,
    Regex,
    Sender,
    SenderAddress,
    Iban,
    TaxId,
    Recipient,
    Folder,
}

impl ConditionKind {
    pub const ALL: [Self; 9] = [
        Self::Substring,
        Self::Word,
        Self::Regex,
        Self::Sender,
        Self::SenderAddress,
        Self::Iban,
        Self::TaxId,
        Self::Recipient,
        Self::Folder,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Substring => "substring",
            Self::Word => "word",
            Self::Regex => "regex",
            Self::Sender => "sender",
            Self::SenderAddress => "sender_address",
            Self::Iban => "iban",
            Self::TaxId => "tax_id",
            Self::Recipient => "recipient",
            Self::Folder => "folder",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, IndexError> {
        match value {
            "substring" => Ok(Self::Substring),
            "word" => Ok(Self::Word),
            "regex" => Ok(Self::Regex),
            "sender" => Ok(Self::Sender),
            "sender_address" => Ok(Self::SenderAddress),
            "iban" => Ok(Self::Iban),
            "tax_id" => Ok(Self::TaxId),
            "recipient" => Ok(Self::Recipient),
            "folder" => Ok(Self::Folder),
            other => Err(corrupt("rule_condition.kind", other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Condition {
    pub kind: ConditionKind,
    pub value: String,
    pub negated: bool,
}

impl Condition {
    pub fn new(kind: ConditionKind, value: impl Into<String>) -> Self {
        Self {
            kind,
            value: value.into(),
            negated: false,
        }
    }

    pub fn not(kind: ConditionKind, value: impl Into<String>) -> Self {
        Self {
            negated: true,
            ..Self::new(kind, value)
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Combinator {
    #[default]
    AllOf,
    AnyOf,
}

impl Combinator {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AllOf => "all",
            Self::AnyOf => "any",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, IndexError> {
        match value {
            "all" => Ok(Self::AllOf),
            "any" => Ok(Self::AnyOf),
            other => Err(corrupt("rule.combinator", other)),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuleConditions {
    pub combinator: Combinator,
    pub list: Vec<Condition>,
}

impl RuleConditions {
    pub fn all_of(list: Vec<Condition>) -> Self {
        Self {
            combinator: Combinator::AllOf,
            list,
        }
    }

    pub fn any_of(list: Vec<Condition>) -> Self {
        Self {
            combinator: Combinator::AnyOf,
            list,
        }
    }

    /// Only under all-of: under any-of a matching rule does not imply the sender.
    pub fn required_sender(&self) -> Option<&str> {
        if self.combinator != Combinator::AllOf {
            return None;
        }
        self.list
            .iter()
            .find(|condition| condition.kind == ConditionKind::Sender && !condition.negated)
            .map(|condition| condition.value.as_str())
    }

    /// One condition of any kind that is not negated and has a value.
    pub fn single_condition(&self) -> Option<&Condition> {
        match self.list.as_slice() {
            [condition] if !condition.negated && !condition.value.trim().is_empty() => {
                Some(condition)
            }
            _ => None,
        }
    }

    pub fn single_text_condition(&self) -> Option<&Condition> {
        match self.list.as_slice() {
            [condition]
                if !condition.negated
                    && matches!(
                        condition.kind,
                        ConditionKind::Substring | ConditionKind::Word
                    ) =>
            {
                Some(condition)
            }
            _ => None,
        }
    }

    pub fn single_text(&self) -> Option<&str> {
        self.single_text_condition()
            .map(|condition| condition.value.as_str())
    }
}

/// The extract patterns are regex sources the index only stores.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleSpec {
    pub conditions: RuleConditions,
    pub extract_number: Option<String>,
    pub extract_date: Option<String>,
    pub extract_amount: Option<String>,
    pub kind: String,
    pub tags: BTreeSet<String>,
    pub filing_pattern: String,
    pub confirmed: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum RuleOrigin {
    #[default]
    Hand,
    Suggestion,
    Filing,
}

impl RuleOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hand => "hand",
            Self::Suggestion => "suggestion",
            Self::Filing => "filing",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, IndexError> {
        match value {
            "hand" => Ok(Self::Hand),
            "suggestion" => Ok(Self::Suggestion),
            "filing" => Ok(Self::Filing),
            other => Err(corrupt("rule.origin", other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub id: RuleId,
    /// Evaluation order, 1-based; changed only by `Index::move_rule`.
    pub position: i64,
    /// Fixed when the rule is created; `Index::update_rule` keeps it.
    pub origin: RuleOrigin,
    pub spec: RuleSpec,
}

impl RuleId {
    /// An id none of `rules` has, for a rule that is only tried and never
    /// stored.
    pub fn unused_in(rules: &[Rule]) -> Self {
        Self(
            rules
                .iter()
                .map(|rule| rule.id.0)
                .max()
                .map_or(1, |highest| highest.saturating_add(1)),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexStatistics {
    pub schema_version: u32,
    pub document_count: u64,
    pub open_reviews: u64,
    pub full_text_bytes: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StatusCounts {
    pub new: u64,
    pub planned: u64,
    pub in_review: u64,
    pub filed: u64,
}

/// What the automatic filing has done so far. `open_reviews_without_text`
/// counts the open review entries whose document has no or an empty text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HermesCounts {
    pub inbox: StatusCounts,
    pub archive: StatusCounts,
    pub open_reviews: u64,
    pub open_reviews_without_text: u64,
    pub confirmed_rules: u64,
    pub archive_documents: u64,
    pub archive_documents_with_text: u64,
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

/// Why a suggested rule was suggested: the condition held for
/// `matching_in_folder` of the `folder_documents` documents of `subtree` and
/// for `matching_elsewhere` documents outside it. `subtree` is `None` for
/// evidence stored before it was kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleEvidence {
    pub matching_in_folder: u32,
    pub folder_documents: u32,
    pub matching_elsewhere: u32,
    pub subtree: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_ibans_pass_the_checksum_in_any_spelling() {
        assert!(iban_is_valid("DE89 3704 0044 0532 0130 00"));
        assert!(iban_is_valid("de89370400440532013000"));
        assert!(iban_is_valid("GB82 WEST 1234 5698 7654 32"));
    }

    #[test]
    fn broken_ibans_fail() {
        assert!(!iban_is_valid("DE89 3704 0044 0532 0130 01"));
        assert!(!iban_is_valid("DE89 3704"));
        assert!(!iban_is_valid("DEXX 3704 0044 0532 0130 00"));
        assert!(!iban_is_valid(""));
    }

    #[test]
    fn the_sender_is_required_only_by_a_plain_sender_condition_under_all_of() {
        let list = vec![
            Condition::not(ConditionKind::Sender, "Agency B"),
            Condition::new(ConditionKind::Substring, "Invoice"),
            Condition::new(ConditionKind::Sender, "Company A"),
        ];

        assert_eq!(
            RuleConditions::all_of(list.clone()).required_sender(),
            Some("Company A")
        );
        assert_eq!(RuleConditions::any_of(list).required_sender(), None);
    }

    #[test]
    fn a_single_condition_is_one_plain_condition_of_any_kind_with_a_value() {
        let sender = Condition::new(ConditionKind::Sender, "Company A");
        assert_eq!(
            RuleConditions::all_of(vec![sender.clone()]).single_condition(),
            Some(&sender)
        );
        for list in [
            vec![Condition::not(ConditionKind::Sender, "Company A")],
            vec![Condition::new(ConditionKind::Sender, "  ")],
            vec![
                sender.clone(),
                Condition::new(ConditionKind::Word, "invoice"),
            ],
            Vec::new(),
        ] {
            assert_eq!(RuleConditions::all_of(list).single_condition(), None);
        }
    }

    #[test]
    fn single_text_is_one_plain_substring_or_word_condition() {
        let substring = Condition::new(ConditionKind::Substring, "Invoice");
        let word = Condition::new(ConditionKind::Word, "Invoice");

        assert_eq!(
            RuleConditions::all_of(vec![substring.clone()]).single_text(),
            Some("Invoice")
        );
        assert_eq!(
            RuleConditions::all_of(vec![word.clone()]).single_text_condition(),
            Some(&word)
        );
        for list in [
            vec![Condition::not(ConditionKind::Substring, "Invoice")],
            vec![Condition::not(ConditionKind::Word, "Invoice")],
            vec![Condition::new(ConditionKind::Regex, "Invoice")],
            vec![Condition::new(ConditionKind::Iban, "Invoice")],
            vec![substring.clone(), substring],
        ] {
            assert_eq!(RuleConditions::all_of(list).single_text(), None);
        }
    }

    #[test]
    fn every_condition_kind_round_trips_through_its_stored_name() {
        for kind in ConditionKind::ALL {
            assert_eq!(ConditionKind::parse(kind.as_str()).ok(), Some(kind));
        }
        assert_eq!(ConditionKind::parse("nonsense").ok(), None);
    }
}
