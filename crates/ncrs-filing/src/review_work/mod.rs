//! The work behind the review list: the documents no confirmed rule could file,
//! and filing one of them by hand. It blocks, so `App` runs it on a blocking
//! thread, never in `update`.
//!
//! What the user types here is a value for a field, never an instruction: a
//! filing pattern is checked and rendered by the same code the rules use, and
//! the move is the journaled executor that nothing overwrites a file with.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use chrono::NaiveDate;
use ncrs_index::{
    Amount, BatchKind, Condition, ConditionKind, Currency, DocumentFields, DocumentId,
    DocumentStatus, Field, FieldOrigin, FieldOrigins, IncludeOutcome, Index, IndexError,
    KindCatalog, KindLanguage, Kinds, ReviewEntryId, ReviewReason, Rule, RuleId, SenderAliases,
};
use ncrs_intake::{
    condition_match_ranges, location_documents, location_structure, plan, relative_to_location,
    render_pattern_in, text_is_readable, NeighbourIndex, OwnNames, Placement, PlanInput, Planning,
};
use ncrs_pipeline::OwnDetails;

use crate::execute::{execute_with, ItemOutcome, SkipReason};
use crate::filing_work::target_of;
use crate::suggestion_work::INDEX_NOT_OPEN;
use ncrs_fs::transfer::{Counting, Move, Source};

use ncrs_ui_kit::format::MINOR_UNITS_PER_UNIT;
pub use ncrs_ui_kit::format::{amount_digits, amount_text};

type IndexSlot = Arc<Mutex<Option<Index>>>;

pub const PATTERN_DOES_NOT_FIT: &str = "the pattern does not fit the fields";
pub const DOCUMENT_EXCLUDED: &str = "the document is excluded from Hermes";
const MINOR_DIGITS: usize = 2;
/// The passages of a document that led to a field or a rule: how many, and how
/// long each may be.
const MAX_PASSAGES: usize = 16;
const MAX_PASSAGE_CHARS: usize = 200;

/// What is known about the text of a document, derived when the list loads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextState {
    Read,
    /// Read or never read, but there is no text.
    NoneFound,
    /// Files of this kind are not read at all, for example images.
    NotReadable,
}

fn text_state(path: &Path, stored: Option<&str>) -> TextState {
    if !text_is_readable(path) {
        TextState::NotReadable
    } else if stored.is_none_or(|text| text.trim().is_empty()) {
        TextState::NoneFound
    } else {
        TextState::Read
    }
}

/// A document waiting for the user, with what the rules could say about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewItem {
    pub entry: ReviewEntryId,
    pub document_id: DocumentId,
    pub path: PathBuf,
    pub reason: ReviewReason,
    pub text_state: TextState,
    /// The stored fields, completed by the rule that came closest.
    pub fields: DocumentFields,
    /// The pattern of that rule, as a start for the one the user confirms.
    pub proposed_pattern: Option<String>,
    /// Where each field of `fields` comes from: the stored origins, and the
    /// rule that came closest for what only it filled in.
    pub origins: FieldOrigins,
    /// The pieces of the text that led to a field or to the rules concerned,
    /// to mark them in the preview.
    pub passages: Vec<String>,
    /// Where documents like this one lie, when the archive says anything.
    pub placement: Option<Placement>,
}

impl ReviewItem {
    /// Where the closest rule would file it, below the archive; `None` when
    /// no rule came close or the fields do not fit its pattern.
    pub fn suggested_target(&self, kinds: &Kinds) -> Option<PathBuf> {
        let pattern = self.proposed_pattern.as_deref()?;
        render_pattern_in(pattern, &self.fields, kinds)
    }
}

/// What accepting the suggestions of several documents came to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AcceptSummary {
    /// The path below the archive of the last document filed.
    pub last_filed: Option<PathBuf>,
    pub filed: usize,
    pub target_exists: usize,
    pub requeued: usize,
    /// No suggestion, or the work failed; the first detail is kept.
    pub failed: usize,
    pub first_failure: Option<String>,
}

impl AcceptSummary {
    fn record(&mut self, outcome: Result<ConfirmOutcome, String>) {
        match outcome {
            Ok(ConfirmOutcome::Filed { relative_path }) => {
                self.filed += 1;
                self.last_filed = Some(relative_path);
            }
            Ok(ConfirmOutcome::TargetExists) => self.target_exists += 1,
            Ok(ConfirmOutcome::Requeued) => self.requeued += 1,
            Err(detail) => {
                self.failed += 1;
                self.first_failure.get_or_insert(detail);
            }
        }
    }

    pub fn total(&self) -> usize {
        self.filed + self.target_exists + self.requeued + self.failed
    }
}

/// What filing a document by hand came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmOutcome {
    /// The path below the archive.
    Filed { relative_path: PathBuf },
    /// The place is taken by another file; the entry stays open and nothing
    /// is overwritten.
    TargetExists,
    /// The file changed or vanished; it goes through the pipeline again.
    Requeued,
}

/// What the review list may do with the index.
#[derive(Clone)]
pub struct ReviewAccess {
    slot: IndexSlot,
    target_root: PathBuf,
    location_roots: Vec<PathBuf>,
    kind_language: KindLanguage,
    own: OwnDetails,
}

impl std::fmt::Debug for ReviewAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReviewAccess").finish_non_exhaustive()
    }
}

impl ReviewAccess {
    pub fn new(slot: IndexSlot, target_root: PathBuf) -> Self {
        Self {
            slot,
            target_root,
            location_roots: Vec::new(),
            kind_language: KindLanguage::default(),
            own: OwnDetails::default(),
        }
    }

    /// The roots of the locations: the rules see a path relative to its root.
    pub fn with_location_roots(self, location_roots: Vec<PathBuf>) -> Self {
        Self {
            location_roots,
            ..self
        }
    }

    pub fn with_kind_language(self, kind_language: KindLanguage) -> Self {
        Self {
            kind_language,
            ..self
        }
    }

    /// What is the user's own and so never says where a document belongs.
    pub fn with_own(self, own: OwnDetails) -> Self {
        Self { own, ..self }
    }

    pub fn load(&self) -> Result<Vec<ReviewItem>, String> {
        self.with_index_text(|index| {
            let neighbours = self.neighbours(index)?;
            load_matching(
                index,
                |_path| true,
                &self.location_roots,
                self.kind_language,
                Some(&neighbours),
            )
            .map_err(|err| err.to_string())
        })
    }

    fn neighbours(&self, index: &Index) -> Result<NeighbourIndex, String> {
        let documents =
            location_documents(index, &self.target_root).map_err(|err| err.to_string())?;
        let structure = location_structure(&documents, index.kinds());
        let own = OwnNames {
            names: &self.own.names,
            ibans: &self.own.ibans,
            tax_ids: &self.own.tax_ids,
        };
        Ok(NeighbourIndex::new(&documents, &own, &structure))
    }

    /// The open reviews of these files, in the order the index lists them.
    pub fn items_for(&self, paths: &[PathBuf]) -> Result<Vec<ReviewItem>, String> {
        self.with_index_text(|index| {
            let neighbours = self.neighbours(index)?;
            load_matching(
                index,
                |path| paths.iter().any(|wanted| wanted == path),
                &self.location_roots,
                self.kind_language,
                Some(&neighbours),
            )
            .map_err(|err| err.to_string())
        })
    }

    /// Files each item under the rule that came closest, one after the other,
    /// through the same journaled move as [`Self::confirm`].
    pub fn accept(&self, items: &[ReviewItem]) -> AcceptSummary {
        let mut summary = AcceptSummary::default();
        for item in items {
            let outcome = match &item.proposed_pattern {
                Some(pattern) => self.confirm(item.entry, item.document_id, &item.fields, pattern),
                None => Err(PATTERN_DOES_NOT_FIT.to_owned()),
            };
            summary.record(outcome);
        }
        summary
    }

    /// Hermes leaves these paths alone from now on.
    pub fn exclude(&self, paths: &[PathBuf]) -> Result<(), String> {
        self.with_index(|index| paths.iter().try_for_each(|path| index.exclude_path(path)))
    }

    /// Hermes looks at this path again. A folder around it that is excluded
    /// stays so.
    pub fn include(&self, path: &Path) -> Result<IncludeOutcome, String> {
        self.with_index(|index| index.include_path(path))
    }

    /// See [`suggestion_for`].
    pub fn suggestion_for(
        &self,
        path: &Path,
    ) -> Result<Option<(TextState, Option<PathBuf>)>, String> {
        self.with_index(|index| {
            suggestion_for(index, path, &self.location_roots, self.kind_language)
        })
    }

    /// Files the document under `pattern` with `fields`, and settles its
    /// entry.
    pub fn confirm(
        &self,
        entry: ReviewEntryId,
        document_id: DocumentId,
        fields: &DocumentFields,
        pattern: &str,
    ) -> Result<ConfirmOutcome, String> {
        let mut guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let index = guard.as_mut().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        confirm(
            index,
            &self.target_root,
            (entry, document_id),
            fields,
            pattern,
            &Move,
            self.kind_language,
        )
    }

    fn with_index_text<T>(
        &self,
        work: impl FnOnce(&mut Index) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let index = guard.as_mut().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        work(index)
    }

    fn with_index<T>(
        &self,
        work: impl FnOnce(&mut Index) -> Result<T, IndexError>,
    ) -> Result<T, String> {
        let mut guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let index = guard.as_mut().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        work(index).map_err(|err| err.to_string())
    }
}

fn load_matching(
    index: &Index,
    wanted: impl Fn(&Path) -> bool,
    location_roots: &[PathBuf],
    kind_language: KindLanguage,
    neighbours: Option<&NeighbourIndex>,
) -> Result<Vec<ReviewItem>, IndexError> {
    let rules = index.rules()?;
    let sender_aliases = index.confirmed_sender_aliases()?;
    let mut items = Vec::new();
    for entry in index.open_reviews()? {
        let Some(document) = index.document(entry.document_id)? else {
            continue;
        };
        if document.missing_since.is_some()
            || !wanted(&document.path)
            || index.is_excluded(&document.path)?
        {
            continue;
        }
        items.push(review_item(
            index,
            &rules,
            &sender_aliases,
            &entry,
            document,
            location_roots,
            kind_language,
            neighbours,
        )?);
    }
    Ok(items)
}

fn review_item(
    index: &Index,
    rules: &[ncrs_index::Rule],
    sender_aliases: &SenderAliases,
    entry: &ncrs_index::ReviewEntry,
    document: ncrs_index::Document,
    location_roots: &[PathBuf],
    kind_language: KindLanguage,
    neighbours: Option<&NeighbourIndex>,
) -> Result<ReviewItem, IndexError> {
    let stored_text = index.document_text(document.id)?;
    let text = stored_text.clone().unwrap_or_default();
    let origins = index.field_origins(document.id)?;
    let manual_fields = origins.manual_fields();
    let kinds = Kinds::new(Arc::clone(index.kinds()), kind_language);
    let planning = plan(
        rules,
        &PlanInput {
            text: &text,
            fields: &document.fields,
            path: Some(relative_to_location(&document.path, location_roots)),
            sender_aliases,
            manual_fields: &manual_fields,
            kinds: &kinds,
        },
    );
    let (fields, proposed_pattern, nearest_rule) = match planning {
        Planning::Review {
            rule: Some(id),
            fields,
            ..
        } => (
            fields,
            rules
                .iter()
                .find(|rule| rule.id == id)
                .map(|rule| rule.spec.filing_pattern.clone()),
            Some(id),
        ),
        Planning::Review { rule: None, .. } | Planning::Plan(_) => {
            (document.fields.clone(), None, None)
        }
    };
    let effective_origins = match nearest_rule {
        Some(id) => with_planned_origins(origins, &document.fields, &fields, id, &kinds.catalog),
        None => origins,
    };
    let passages = passages_of(rules, &effective_origins, nearest_rule, &fields, &text);
    let placement = neighbours.and_then(|known| known.place(&fields, stored_text.as_deref()));
    let rule_or_placement_pattern =
        proposed_pattern.or_else(|| placement_pattern(placement.as_ref()?, &fields, &kinds));
    Ok(ReviewItem {
        entry: entry.id,
        document_id: document.id,
        text_state: text_state(&document.path, stored_text.as_deref()),
        path: document.path,
        reason: entry.reason,
        fields,
        proposed_pattern: rule_or_placement_pattern,
        origins: effective_origins,
        passages,
        placement,
    })
}

/// The pattern of the folder where similar documents lie, when that is sure
/// and it renders for these fields.
fn placement_pattern(
    placement: &Placement,
    fields: &DocumentFields,
    kinds: &Kinds,
) -> Option<String> {
    if !placement.is_sure() || placement.missing.is_some() {
        return None;
    }
    let pattern = placement.pattern.as_deref()?;
    render_pattern_in(pattern, fields, kinds)?;
    Some(pattern.to_owned())
}

/// The stored origins, but a field only the closest rule filled in (its
/// planned value is not the stored one) is that rule's.
fn with_planned_origins(
    stored: FieldOrigins,
    stored_fields: &DocumentFields,
    planned_fields: &DocumentFields,
    rule: RuleId,
    kinds: &KindCatalog,
) -> FieldOrigins {
    Field::ALL
        .into_iter()
        .filter(|field| !same_value(stored_fields, planned_fields, *field, kinds))
        .fold(stored, |origins, field| {
            origins.with(field, FieldOrigin::Rule(Some(rule)))
        })
}

fn same_value(
    left: &DocumentFields,
    right: &DocumentFields,
    field: Field,
    kinds: &KindCatalog,
) -> bool {
    match field {
        Field::Sender => left.sender == right.sender,
        Field::Recipient => left.recipient == right.recipient,
        Field::Kind => {
            left.kind.as_deref().map(|kind| kinds.kind_key(kind))
                == right.kind.as_deref().map(|kind| kinds.kind_key(kind))
        }
        Field::Date => left.date == right.date,
        Field::Number => left.number == right.number,
        Field::Amount => left.amount == right.amount,
    }
}

/// What in `text` led to the fields and to the rules concerned: the matches
/// of the conditions of the closest rule and of the rules that gave a field,
/// and the sender and number where a rule or the AI read them.
fn passages_of(
    rules: &[Rule],
    origins: &FieldOrigins,
    nearest_rule: Option<RuleId>,
    fields: &DocumentFields,
    text: &str,
) -> Vec<String> {
    let mut rule_ids: Vec<RuleId> = nearest_rule.into_iter().collect();
    for field in Field::ALL {
        if let FieldOrigin::Rule(Some(id)) = origins.of(field) {
            if !rule_ids.contains(&id) {
                rule_ids.push(id);
            }
        }
    }
    let rule_conditions = rule_ids
        .iter()
        .filter_map(|id| rules.iter().find(|rule| rule.id == *id))
        .flat_map(|rule| rule.spec.conditions.list.iter().cloned());
    let read_values = [
        (Field::Sender, &fields.sender),
        (Field::Number, &fields.number),
    ]
    .into_iter()
    .filter(|(field, _)| {
        matches!(
            origins.of(*field),
            FieldOrigin::Rule(_) | FieldOrigin::Ai { .. }
        )
    })
    .filter_map(|(_, value)| value.as_deref())
    .map(|value| Condition::new(ConditionKind::Word, value));

    let mut passages: Vec<String> = Vec::new();
    for condition in rule_conditions.chain(read_values) {
        for range in condition_match_ranges(&condition, text) {
            let Some(found) = text.get(range) else {
                continue;
            };
            let passage: String = found.trim().chars().take(MAX_PASSAGE_CHARS).collect();
            if !passage.is_empty() && !passages.contains(&passage) {
                passages.push(passage);
            }
            if passages.len() >= MAX_PASSAGES {
                return passages;
            }
        }
    }
    passages
}

/// The review of one file, for the status bar: how its text stands and where
/// the closest rule would file it. `None` when the file has no open review.
fn suggestion_for(
    index: &Index,
    path: &Path,
    location_roots: &[PathBuf],
    kind_language: KindLanguage,
) -> Result<Option<(TextState, Option<PathBuf>)>, IndexError> {
    let Some(document) = index.document_by_path(path)? else {
        return Ok(None);
    };
    let Some(entry) = index.open_review_of(document.id)? else {
        return Ok(None);
    };
    let rules = index.rules()?;
    let sender_aliases = index.confirmed_sender_aliases()?;
    let item = review_item(
        index,
        &rules,
        &sender_aliases,
        &entry,
        document,
        location_roots,
        kind_language,
        None,
    )?;
    let kinds = Kinds::new(Arc::clone(index.kinds()), kind_language);
    let target = item.suggested_target(&kinds);
    Ok(Some((item.text_state, target)))
}

fn confirm<S: Source>(
    index: &mut Index,
    root: &Path,
    (entry, document_id): (ReviewEntryId, DocumentId),
    fields: &DocumentFields,
    pattern: &str,
    effect: &S,
    kind_language: KindLanguage,
) -> Result<ConfirmOutcome, String> {
    let failed = |err: IndexError| err.to_string();
    let kinds = Kinds::new(Arc::clone(index.kinds()), kind_language);
    let relative = render_pattern_in(pattern, fields, &kinds)
        .ok_or_else(|| PATTERN_DOES_NOT_FIT.to_owned())?;
    let document = index
        .document(document_id)
        .map_err(failed)?
        .ok_or_else(|| "the document is no longer in the index".to_owned())?;
    if index.is_excluded(&document.path).map_err(failed)? {
        return Err(DOCUMENT_EXCLUDED.to_owned());
    }
    let target = target_of(root, &relative, &document.path);
    let item = crate::execute::ExecutionItem {
        from: document.path.clone(),
        to: target.clone(),
        expected_hash: document.hash,
        document_id: Some(document_id),
        duplicate: false,
    };
    let report = execute_with(
        index,
        BatchKind::Filing,
        root,
        &[item],
        &mut Counting,
        effect,
    )
    .map_err(|err| err.to_string())?;
    let outcome = report.items.first().map(|reported| &reported.outcome);
    match outcome {
        Some(ItemOutcome::Moved) => {
            index
                .set_document_fields(document_id, fields, |_| FieldOrigin::Manual)
                .map_err(failed)?;
            index.resolve_review(entry).map_err(failed)?;
            let relative_path = target
                .strip_prefix(root)
                .map_or_else(|_| target.clone(), Path::to_path_buf);
            Ok(ConfirmOutcome::Filed { relative_path })
        }
        Some(ItemOutcome::Skipped(SkipReason::TargetExists)) => Ok(ConfirmOutcome::TargetExists),
        Some(ItemOutcome::Skipped(SkipReason::SourceChanged | SkipReason::SourceMissing)) => {
            index.resolve_review(entry).map_err(failed)?;
            index
                .set_document_status(document_id, DocumentStatus::New)
                .map_err(failed)?;
            Ok(ConfirmOutcome::Requeued)
        }
        Some(ItemOutcome::Skipped(other)) => Err(format!("the file was not moved: {other:?}")),
        None => Err("the file was not moved".to_owned()),
    }
}

/// A field the user typed that is not what the field holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldError {
    Date,
    Amount,
}

/// `YYYY-MM-DD`; nothing typed is no date.
pub fn parse_date(typed: &str) -> Result<Option<NaiveDate>, FieldError> {
    let text = typed.trim();
    if text.is_empty() {
        return Ok(None);
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .map(Some)
        .map_err(|_parse_error| FieldError::Date)
}

/// `12.34 EUR` or `12,34 EUR`; nothing typed is no amount.
pub fn parse_amount(typed: &str) -> Result<Option<Amount>, FieldError> {
    let text = typed.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let mut words = text.split_whitespace();
    let (Some(number_word), Some(code), None) = (words.next(), words.next(), words.next()) else {
        return Err(FieldError::Amount);
    };
    let currency = Currency::new(&code.to_ascii_uppercase()).ok_or(FieldError::Amount)?;
    let (negative, number) = match number_word.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, number_word),
    };
    let (whole, fraction) = number
        .split_once(['.', ','])
        .map_or((number, ""), |(whole, fraction)| (whole, fraction));
    let digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    if whole.is_empty() || !digits(whole) || !digits(fraction) || fraction.len() > MINOR_DIGITS {
        return Err(FieldError::Amount);
    }
    let padded = format!("{fraction:0<MINOR_DIGITS$}");
    let minor = |part: &str| {
        part.parse::<i64>()
            .map_err(|_parse_error| FieldError::Amount)
    };
    let magnitude = minor(whole)?
        .checked_mul(MINOR_UNITS_PER_UNIT)
        .and_then(|units| units.checked_add(minor(&padded).ok()?))
        .ok_or(FieldError::Amount)?;
    Ok(Some(Amount {
        minor_units: if negative { -magnitude } else { magnitude },
        currency,
    }))
}

#[cfg(test)]
mod tests;
