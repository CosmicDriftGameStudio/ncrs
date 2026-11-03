use std::collections::BTreeSet;
use std::fmt;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, NaiveDate, Utc};

use crate::error::IndexError;
use crate::kinds::KindCatalog;

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
    Recipient,
    Kind,
    Date,
    Number,
    Amount,
}

impl Field {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sender => "sender",
            Self::Recipient => "recipient",
            Self::Kind => "kind",
            Self::Date => "date",
            Self::Number => "number",
            Self::Amount => "amount",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, IndexError> {
        match value {
            "sender" => Ok(Self::Sender),
            "recipient" => Ok(Self::Recipient),
            "kind" => Ok(Self::Kind),
            "date" => Ok(Self::Date),
            "number" => Ok(Self::Number),
            "amount" => Ok(Self::Amount),
            other => Err(corrupt("review_entry.field", other)),
        }
    }
}

impl Field {
    pub const ALL: [Self; 6] = [
        Self::Sender,
        Self::Recipient,
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
    /// The recipient was read from the address block of the letter.
    AddressBlock,
    /// The kind was read from the heading of the letter.
    Heading,
}

impl FieldOrigin {
    pub(crate) fn kind_column(&self) -> Option<&'static str> {
        match self {
            Self::Unknown => None,
            Self::Rule(_) => Some("rule"),
            Self::Ai { .. } => Some("ai"),
            Self::Manual => Some("manual"),
            Self::AddressBlock => Some("address"),
            Self::Heading => Some("heading"),
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
            "address" => Ok(Self::AddressBlock),
            "heading" => Ok(Self::Heading),
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

/// The spelling of a tag: lowercase words joined by single hyphens.
pub fn tag_name(text: &str) -> String {
    let mut name = String::new();
    let mut pending_hyphen = false;
    for character in text.trim().chars() {
        if character.is_alphanumeric() {
            if pending_hyphen && !name.is_empty() {
                name.push('-');
            }
            pending_hyphen = false;
            name.extend(character.to_lowercase());
        } else {
            pending_hyphen = true;
        }
    }
    name
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentFields {
    pub sender: Option<String>,
    pub recipient: Option<String>,
    pub kind: Option<String>,
    pub date: Option<NaiveDate>,
    pub number: Option<String>,
    pub amount: Option<Amount>,
    pub tags: BTreeSet<String>,
}

impl DocumentFields {
    /// The tags the fields imply: kind, sender, recipient and year.
    pub fn automatic_tags(&self, kinds: &KindCatalog) -> BTreeSet<String> {
        let year = self.date.map(|date| date.format("%Y").to_string());
        let kind = self.kind.as_deref().map(|kind| kinds.kind_tag(kind));
        let others = [
            self.sender.as_deref(),
            self.recipient.as_deref(),
            year.as_deref(),
        ]
        .into_iter()
        .flatten()
        .map(tag_name);
        kind.into_iter()
            .chain(others)
            .filter(|name| !name.is_empty())
            .collect()
    }

    /// The tags the user set, normalised.
    pub fn own_tags(&self) -> BTreeSet<String> {
        self.tags
            .iter()
            .map(|tag| tag_name(tag))
            .filter(|name| !name.is_empty())
            .collect()
    }

    pub fn all_tags(&self, kinds: &KindCatalog) -> BTreeSet<String> {
        let mut all = self.own_tags();
        all.extend(self.automatic_tags(kinds));
        all
    }
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
    /// The document has a tag (own or automatic) that is this one or below it.
    Tag,
    /// The kind field of the document is this kind; the value is the kind key.
    Kind,
}

impl ConditionKind {
    pub const ALL: [Self; 11] = [
        Self::Substring,
        Self::Word,
        Self::Regex,
        Self::Sender,
        Self::SenderAddress,
        Self::Iban,
        Self::TaxId,
        Self::Recipient,
        Self::Folder,
        Self::Tag,
        Self::Kind,
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
            Self::Tag => "tag",
            Self::Kind => "kind",
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
            "tag" => Ok(Self::Tag),
            "kind" => Ok(Self::Kind),
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
    /// What the user calls the rule; shown in lists.
    pub name: Option<String>,
    pub conditions: RuleConditions,
    pub extract_number: Option<String>,
    pub extract_date: Option<String>,
    pub extract_amount: Option<String>,
    /// A rule without a kind sets none.
    pub kind: Option<String>,
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

mod reorganisation;
pub use reorganisation::{
    ConflictReason, Finding, FindingKind, NoActionReason, PlanItem, PlanOutcome, ReorganisationPlan,
};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tag_tests;
