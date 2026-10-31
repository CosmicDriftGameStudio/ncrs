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
    condition_match_ranges, plan, render_pattern_in, text_is_readable, PlanInput, Planning,
};

use crate::execute::{execute_with, ItemOutcome, SkipReason};
use crate::filing_work::target_of;
use crate::fs::transfer::{Counting, Move, Source};
use crate::suggestion_work::INDEX_NOT_OPEN;

type IndexSlot = Arc<Mutex<Option<Index>>>;

pub const PATTERN_DOES_NOT_FIT: &str = "the pattern does not fit the fields";
const MINOR_UNITS_PER_UNIT: i64 = 100;
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
    kind_language: KindLanguage,
}

impl ReviewAccess {
    pub fn new(slot: IndexSlot, target_root: PathBuf) -> Self {
        Self {
            slot,
            target_root,
            kind_language: KindLanguage::default(),
        }
    }

    pub fn with_kind_language(self, kind_language: KindLanguage) -> Self {
        Self {
            kind_language,
            ..self
        }
    }

    pub fn load(&self) -> Result<Vec<ReviewItem>, String> {
        self.with_index(|index| load(index, self.kind_language))
    }

    /// The open reviews of these files, in the order the index lists them.
    pub fn items_for(&self, paths: &[PathBuf]) -> Result<Vec<ReviewItem>, String> {
        self.with_index(|index| {
            load_matching(
                index,
                |path| paths.iter().any(|wanted| wanted == path),
                self.kind_language,
            )
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
        self.with_index(|index| suggestion_for(index, path, self.kind_language))
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

    fn with_index<T>(
        &self,
        work: impl FnOnce(&mut Index) -> Result<T, IndexError>,
    ) -> Result<T, String> {
        let mut guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let index = guard.as_mut().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        work(index).map_err(|err| err.to_string())
    }
}

fn load(index: &Index, kind_language: KindLanguage) -> Result<Vec<ReviewItem>, IndexError> {
    load_matching(index, |_path| true, kind_language)
}

fn load_matching(
    index: &Index,
    wanted: impl Fn(&Path) -> bool,
    kind_language: KindLanguage,
) -> Result<Vec<ReviewItem>, IndexError> {
    let rules = index.rules()?;
    let sender_aliases = index.confirmed_sender_aliases()?;
    let mut items = Vec::new();
    for entry in index.open_reviews()? {
        let Some(document) = index.document(entry.document_id)? else {
            continue;
        };
        if !wanted(&document.path) {
            continue;
        }
        items.push(review_item(
            index,
            &rules,
            &sender_aliases,
            &entry,
            document,
            kind_language,
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
    kind_language: KindLanguage,
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
            path: Some(&document.path),
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
    Ok(ReviewItem {
        entry: entry.id,
        document_id: document.id,
        text_state: text_state(&document.path, stored_text.as_deref()),
        path: document.path,
        reason: entry.reason,
        fields,
        proposed_pattern,
        origins: effective_origins,
        passages,
    })
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
    kind_language: KindLanguage,
) -> Result<Option<(TextState, Option<PathBuf>)>, IndexError> {
    let rules = index.rules()?;
    let sender_aliases = index.confirmed_sender_aliases()?;
    for entry in index.open_reviews()? {
        let Some(document) = index.document(entry.document_id)? else {
            continue;
        };
        if document.path != path {
            continue;
        }
        let item = review_item(
            index,
            &rules,
            &sender_aliases,
            &entry,
            document,
            kind_language,
        )?;
        let kinds = Kinds::new(Arc::clone(index.kinds()), kind_language);
        let target = item
            .proposed_pattern
            .and_then(|pattern| render_pattern_in(&pattern, &item.fields, &kinds));
        return Ok(Some((item.text_state, target)));
    }
    Ok(None)
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
    index
        .set_document_fields(document_id, fields, |_| FieldOrigin::Manual)
        .map_err(failed)?;
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

/// The amount as the field shows it, parseable by [`parse_amount`].
pub fn amount_text(amount: &Amount) -> String {
    format!("{} {}", amount_digits(amount), amount.currency.as_str())
}

/// The number of an amount without its currency, with two decimals.
pub fn amount_digits(amount: &Amount) -> String {
    let magnitude = amount.minor_units.unsigned_abs();
    let unit = MINOR_UNITS_PER_UNIT.unsigned_abs();
    let sign = if amount.minor_units < 0 { "-" } else { "" };
    format!(
        "{sign}{}.{:02}",
        magnitude.div_euclid(unit),
        magnitude.rem_euclid(unit)
    )
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;

    use chrono::{TimeZone as _, Utc};
    use ncrs_index::{
        Condition, ConditionKind, DocumentSource, NewDocument, RuleConditions, RuleSpec,
    };
    use ncrs_intake::hash_path;
    use tempfile::TempDir;

    use super::*;
    use crate::filing_work::file_planned;

    struct Setup {
        _dir: TempDir,
        inbox: PathBuf,
        archive: PathBuf,
        slot: IndexSlot,
    }

    impl Setup {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let inbox = dir.path().join("inbox");
            let archive = dir.path().join("archive");
            fs::create_dir_all(&inbox).unwrap();
            fs::create_dir_all(&archive).unwrap();
            let index = Index::create_plain(&dir.path().join("index.db")).unwrap();
            Self {
                _dir: dir,
                inbox,
                archive,
                slot: Arc::new(Mutex::new(Some(index))),
            }
        }

        fn access(&self) -> ReviewAccess {
            ReviewAccess::new(Arc::clone(&self.slot), self.archive.clone())
        }

        fn with_index<T>(&self, work: impl FnOnce(&mut Index) -> T) -> T {
            let mut guard = self.slot.lock().unwrap();
            work(guard.as_mut().unwrap())
        }

        /// An inbox file in `status`; in review when no rule placed it.
        fn document(&self, name: &str, status: DocumentStatus) -> DocumentId {
            let path = self.inbox.join(name);
            fs::write(&path, format!("content of {name}")).unwrap();
            let metadata = fs::metadata(&path).unwrap();
            self.with_index(|index| {
                let id = index
                    .insert_document(&NewDocument {
                        hash: hash_path(&path).unwrap(),
                        path: path.clone(),
                        source: DocumentSource::Inbox,
                        received_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
                        stamp: Some(ncrs_index::FileStamp {
                            size: metadata.len(),
                            mtime: metadata.modified().unwrap(),
                        }),
                    })
                    .unwrap();
                index.set_document_text(id, "Invoice").unwrap();
                index.set_document_status(id, status).unwrap();
                id
            })
        }

        fn in_review(&self, name: &str) -> DocumentId {
            let id = self.document(name, DocumentStatus::InReview);
            self.with_index(|index| index.add_review(id, ReviewReason::NoMatchingRule))
                .unwrap();
            id
        }

        fn fields(sender: &str) -> DocumentFields {
            DocumentFields {
                sender: Some(sender.to_owned()),
                kind: Some("invoice".to_owned()),
                ..DocumentFields::default()
            }
        }
    }

    fn first(access: &ReviewAccess) -> ReviewItem {
        access.load().unwrap().remove(0)
    }

    #[test]
    fn the_text_state_tells_images_from_empty_from_read_text() {
        let setup = Setup::new();
        let read = setup.in_review("invoice.pdf");
        let empty = setup.in_review("blank.pdf");
        let image = setup.in_review("photo.jpg");
        let never_read = setup.in_review("never.pdf");
        setup.with_index(|index| {
            index.set_document_text(empty, "").unwrap();
            index.set_document_text(image, "").unwrap();
            index
                .connection()
                .execute(
                    "DELETE FROM document_text
                     WHERE rowid = (SELECT id FROM document WHERE path LIKE '%never.pdf')",
                    [],
                )
                .unwrap();
        });

        let states: Vec<(DocumentId, TextState)> = setup
            .access()
            .load()
            .unwrap()
            .into_iter()
            .map(|item| (item.document_id, item.text_state))
            .collect();

        assert_eq!(
            states,
            vec![
                (read, TextState::Read),
                (empty, TextState::NoneFound),
                (image, TextState::NotReadable),
                (never_read, TextState::NoneFound),
            ]
        );
    }

    #[test]
    fn a_document_no_rule_filed_is_listed_with_its_reason() {
        let setup = Setup::new();
        let id = setup.in_review("scan.pdf");

        let item = first(&setup.access());

        assert_eq!(item.document_id, id);
        assert_eq!(item.reason, ReviewReason::NoMatchingRule);
        assert_eq!(item.proposed_pattern, None);
    }

    #[test]
    fn confirming_with_changed_fields_files_the_document_where_the_pattern_says() {
        let setup = Setup::new();
        setup.in_review("scan.pdf");
        let access = setup.access();
        let item = first(&access);

        let outcome = access
            .confirm(
                item.entry,
                item.document_id,
                &Setup::fields("Company A"),
                "{sender}/{kind}",
            )
            .unwrap();

        assert_eq!(
            outcome,
            ConfirmOutcome::Filed {
                relative_path: PathBuf::from("Company A/Invoice.pdf")
            }
        );
        let target = setup.archive.join("Company A").join("Invoice.pdf");
        assert_eq!(fs::read_to_string(target).unwrap(), "content of scan.pdf");
        assert!(!setup.inbox.join("scan.pdf").exists());
        setup.with_index(|index| {
            let document = index.document(item.document_id).unwrap().unwrap();
            assert_eq!(document.status, DocumentStatus::Filed);
            assert_eq!(document.fields.sender.as_deref(), Some("Company A"));
            assert!(index.open_reviews().unwrap().is_empty());
            let origins = index.field_origins(item.document_id).unwrap();
            for field in [ncrs_index::Field::Sender, ncrs_index::Field::Kind] {
                assert_eq!(origins.of(field), FieldOrigin::Manual, "{field:?}");
            }
            assert_eq!(origins.of(ncrs_index::Field::Date), FieldOrigin::Unknown);
        });
    }

    #[test]
    fn a_rule_for_the_sender_files_the_next_document_without_a_review() {
        let setup = Setup::new();
        setup.with_index(|index| {
            let spec = RuleSpec {
                name: None,
                conditions: RuleConditions::all_of(vec![Condition::new(
                    ConditionKind::Sender,
                    "Company A",
                )]),
                extract_number: None,
                extract_date: None,
                extract_amount: None,
                kind: Some("invoice".to_owned()),
                tags: BTreeSet::new(),
                filing_pattern: "{sender}/{kind}".to_owned(),
                confirmed: true,
                enabled: true,
            };
            index
                .create_rule_from(&spec, ncrs_index::RuleOrigin::Filing)
                .unwrap();
        });
        let next = setup.document("next.pdf", DocumentStatus::Planned);
        setup.with_index(|index| {
            index
                .set_document_fields(next, &Setup::fields("Company A"), |_| FieldOrigin::Unknown)
                .unwrap();
        });

        let summary = setup.with_index(|index| {
            file_planned(
                index,
                &setup.archive,
                &mut Counting,
                &Move,
                KindLanguage::English,
            )
            .unwrap()
        });

        assert_eq!(summary.filed, 1);
        assert_eq!(summary.reviewed, 0);
        assert!(setup.archive.join("Company A").join("Invoice.pdf").exists());
        setup.with_index(|index| assert!(index.open_reviews().unwrap().is_empty()));
    }

    #[test]
    fn a_taken_place_keeps_the_entry_open_and_the_target_unchanged() {
        let setup = Setup::new();
        setup.in_review("scan.pdf");
        let taken = setup.archive.join("Company A").join("Invoice.pdf");
        fs::create_dir_all(taken.parent().unwrap()).unwrap();
        fs::write(&taken, "the other file").unwrap();
        let access = setup.access();
        let item = first(&access);

        let outcome = access
            .confirm(
                item.entry,
                item.document_id,
                &Setup::fields("Company A"),
                "{sender}/{kind}",
            )
            .unwrap();

        assert_eq!(outcome, ConfirmOutcome::TargetExists);
        assert_eq!(fs::read_to_string(&taken).unwrap(), "the other file");
        assert!(setup.inbox.join("scan.pdf").exists());
        assert_eq!(access.load().unwrap().len(), 1);
    }

    #[test]
    fn a_pattern_that_cannot_be_rendered_moves_nothing() {
        let setup = Setup::new();
        setup.in_review("scan.pdf");
        let access = setup.access();
        let item = first(&access);

        for pattern in ["{nonsense}", "{date}/{kind}", "../{kind}", ""] {
            let result = access.confirm(
                item.entry,
                item.document_id,
                &Setup::fields("Company A"),
                pattern,
            );

            assert_eq!(result, Err(PATTERN_DOES_NOT_FIT.to_owned()), "{pattern}");
        }
        assert!(setup.inbox.join("scan.pdf").exists());
        assert_eq!(access.load().unwrap().len(), 1);
    }

    #[test]
    fn a_file_that_vanished_goes_back_to_the_pipeline() {
        let setup = Setup::new();
        setup.in_review("scan.pdf");
        let access = setup.access();
        let item = first(&access);
        fs::remove_file(setup.inbox.join("scan.pdf")).unwrap();

        let outcome = access
            .confirm(
                item.entry,
                item.document_id,
                &Setup::fields("Company A"),
                "{sender}/{kind}",
            )
            .unwrap();

        assert_eq!(outcome, ConfirmOutcome::Requeued);
        setup.with_index(|index| {
            let document = index.document(item.document_id).unwrap().unwrap();
            assert_eq!(document.status, DocumentStatus::New);
            assert!(index.open_reviews().unwrap().is_empty());
        });
    }

    #[test]
    fn a_rule_that_matches_the_text_gives_its_passages_and_the_fields_it_filled() {
        let setup = Setup::new();
        let document = setup.in_review("rule.pdf");
        let rule = setup.with_index(|index| {
            index
                .set_document_text(document, "Invoice 4711\nfrom COMPANY A Ltd")
                .unwrap();
            let spec = RuleSpec {
                name: None,
                conditions: RuleConditions::all_of(vec![
                    Condition::new(ConditionKind::Substring, "company a"),
                    Condition::not(ConditionKind::Substring, "agency"),
                ]),
                extract_number: None,
                extract_date: None,
                extract_amount: None,
                kind: Some("invoice".to_owned()),
                tags: BTreeSet::new(),
                filing_pattern: "{kind}".to_owned(),
                confirmed: false,
                enabled: true,
            };
            index
                .create_rule_from(&spec, ncrs_index::RuleOrigin::Filing)
                .unwrap()
        });

        let item = first(&setup.access());

        assert_eq!(item.passages, ["COMPANY A"]);
        assert_eq!(item.origins.of(Field::Kind), FieldOrigin::Rule(Some(rule)));
        assert_eq!(item.origins.of(Field::Sender), FieldOrigin::Unknown);
    }

    #[test]
    fn the_sender_the_ai_read_is_a_passage_where_it_stands_in_the_text() {
        let setup = Setup::new();
        let document = setup.in_review("ai.pdf");
        setup.with_index(|index| {
            index
                .set_document_text(document, "Invoice 4711\nCompany A Ltd")
                .unwrap();
            index
                .set_document_fields(document, &Setup::fields("Company A"), |field| match field {
                    Field::Sender => FieldOrigin::Ai {
                        confidence_percent: 82,
                    },
                    _ => FieldOrigin::Manual,
                })
                .unwrap();
        });

        let item = first(&setup.access());

        assert_eq!(item.passages, ["Company A"]);
        assert_eq!(
            item.origins.of(Field::Sender),
            FieldOrigin::Ai {
                confidence_percent: 82
            }
        );
    }

    #[test]
    fn dates_are_iso_and_empty_is_none() {
        assert_eq!(parse_date(""), Ok(None));
        assert_eq!(
            parse_date(" 2024-03-09 "),
            Ok(NaiveDate::from_ymd_opt(2024, 3, 9))
        );
        for bad in ["09.03.2024", "2024-13-01", "yesterday"] {
            assert_eq!(parse_date(bad), Err(FieldError::Date), "{bad}");
        }
    }

    #[test]
    fn amounts_take_a_point_or_a_comma_and_a_currency() {
        let eur = |minor_units| {
            Ok(Some(Amount {
                minor_units,
                currency: Currency::new("EUR").unwrap(),
            }))
        };

        assert_eq!(parse_amount(""), Ok(None));
        assert_eq!(parse_amount("12.34 EUR"), eur(1234));
        assert_eq!(parse_amount("12,34 eur"), eur(1234));
        assert_eq!(parse_amount("12 EUR"), eur(1200));
        assert_eq!(parse_amount("0.5 EUR"), eur(50));
        assert_eq!(parse_amount("-3.05 EUR"), eur(-305));
        for bad in [
            "12.34",
            "EUR",
            "12.345 EUR",
            "1x EUR",
            "12 EURO",
            "12 EUR now",
            ".5 EUR",
        ] {
            assert_eq!(parse_amount(bad), Err(FieldError::Amount), "{bad}");
        }
        assert_eq!(
            parse_amount("99999999999999999999 EUR"),
            Err(FieldError::Amount)
        );
    }

    #[test]
    fn an_amount_shown_is_an_amount_that_parses() {
        let amount = parse_amount("-7,05 EUR").unwrap().unwrap();

        assert_eq!(amount_text(&amount), "-7.05 EUR");
        assert_eq!(parse_amount(&amount_text(&amount)), Ok(Some(amount)));
    }
}
