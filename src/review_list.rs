//! State of the review list: the documents waiting for the user, the cursor in
//! them and the form with the fields of the one under it. No widgets and no
//! tasks; `App` owns it and changes it from `update`, the view is
//! `ui::review_list`.

use std::collections::BTreeSet;
use std::path::PathBuf;

use iced::mouse::ScrollDelta;

use ncrs_index::{DocumentFields, DocumentId, Field, ReviewEntryId, TaxContext, TaxContextEntry};
use ncrs_intake::{
    choose_pattern, default_pattern_for, missing_field, render_pattern, render_pattern_parts,
    PatternPart, PatternSource,
};

use crate::document_preview::{DocumentPreview, PreviewContent, PreviewRequest};
use crate::list_window::ListWindow;
use crate::replan_work::{ReplanItem, ReplanScope};
use crate::review_work::{
    amount_text, parse_amount, parse_date, parse_tags, tags_text, ConfirmOutcome, FieldError,
    ReviewItem,
};
use crate::rule_form::RulePrefill;

/// A text field of the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormField {
    Sender,
    Kind,
    Date,
    Number,
    Amount,
    Tags,
    Pattern,
}

/// The fields as the user sees and edits them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Form {
    pub sender: String,
    pub kind: String,
    /// `None` is a context nobody has chosen yet.
    pub tax_context: Option<TaxContext>,
    pub date: String,
    pub number: String,
    pub amount: String,
    pub tags: String,
    pub pattern: String,
}

impl Form {
    fn of(item: &ReviewItem, custom_default: Option<&str>) -> Self {
        let fields = &item.fields;
        Self {
            sender: fields.sender.clone().unwrap_or_default(),
            kind: fields.kind.clone().unwrap_or_default(),
            tax_context: fields.tax_context.clone(),
            date: fields.date.map(|date| date.to_string()).unwrap_or_default(),
            number: fields.number.clone().unwrap_or_default(),
            amount: fields.amount.as_ref().map(amount_text).unwrap_or_default(),
            tags: tags_text(&fields.tags),
            pattern: choose_pattern(item.proposed_pattern.as_deref(), custom_default, fields)
                .map(|chosen| chosen.pattern)
                .unwrap_or_default(),
        }
    }

    fn text_mut(&mut self, field: FormField) -> &mut String {
        match field {
            FormField::Sender => &mut self.sender,
            FormField::Kind => &mut self.kind,
            FormField::Date => &mut self.date,
            FormField::Number => &mut self.number,
            FormField::Amount => &mut self.amount,
            FormField::Tags => &mut self.tags,
            FormField::Pattern => &mut self.pattern,
        }
    }

    /// The fields the form holds; what is typed and no field can hold is a
    /// problem, never silently dropped.
    fn fields(&self) -> Result<DocumentFields, Problem> {
        let non_empty = |typed: &str| {
            let text = typed.trim();
            (!text.is_empty()).then(|| text.to_owned())
        };
        Ok(DocumentFields {
            sender: non_empty(&self.sender),
            kind: non_empty(&self.kind),
            tax_context: self.tax_context.clone(),
            date: parse_date(&self.date).map_err(Problem::from)?,
            number: non_empty(&self.number),
            amount: parse_amount(&self.amount).map_err(Problem::from)?,
            tags: parse_tags(&self.tags),
        })
    }
}

/// Why nothing was filed, or what the user should know about how it went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    InvalidDate,
    InvalidAmount,
    /// The pattern is invalid, or needs a field that is empty.
    PatternDoesNotFit,
    /// The place the pattern gives is taken; nothing was overwritten.
    TargetExists,
    /// The file changed or vanished; it goes through the pipeline again.
    FileChanged,
    /// The work on the index failed; the technical detail stays English.
    Failed(String),
}

impl From<FieldError> for Problem {
    fn from(error: FieldError) -> Self {
        match error {
            FieldError::Date => Self::InvalidDate,
            FieldError::Amount => Self::InvalidAmount,
        }
    }
}

/// What was filed last and what a rule for its sender would be made of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FiledNotice {
    /// Below the archive.
    pub path: PathBuf,
    pub sender: String,
    pub kind: String,
    pub tax_context: Option<TaxContext>,
    pub tags: BTreeSet<String>,
    pub pattern: String,
}

impl FiledNotice {
    /// A rule needs something to recognise the sender by.
    pub fn offers_rule(&self) -> bool {
        !self.sender.is_empty() && !self.kind.is_empty()
    }

    fn rule_prefill(&self) -> RulePrefill {
        RulePrefill {
            sender: self.sender.clone(),
            kind: self.kind.clone(),
            tax_context: self.tax_context.clone(),
            tags: self.tags.clone(),
            pattern: self.pattern.clone(),
        }
    }
}

/// What a rule form for `item` starts with: its fields, and the pattern the
/// review form would show for it.
pub fn rule_prefill_of(item: &ReviewItem, custom_default: Option<&str>) -> RulePrefill {
    let fields = &item.fields;
    RulePrefill {
        sender: fields.sender.clone().unwrap_or_default(),
        kind: fields.kind.clone().unwrap_or_default(),
        tax_context: fields.tax_context.clone(),
        tags: fields.tags.clone(),
        pattern: Form::of(item, custom_default).pattern,
    }
}

/// What the work needs to file the selected document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmRequest {
    pub entry: ReviewEntryId,
    pub document_id: DocumentId,
    pub fields: DocumentFields,
    pub pattern: String,
}

/// The step before documents are filed again with the current rules.
#[derive(Debug, Clone, PartialEq)]
pub enum ReplanStep {
    /// The documents are being planned again; nothing is shown yet.
    Looking(ReplanScope),
    /// What the rules would do now. Nothing has moved.
    Preview {
        scope: ReplanScope,
        items: Vec<ReplanItem>,
    },
}

/// Most entries of the list shown at once; the list follows the cursor.
pub const VISIBLE_ROWS: usize = 14;

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewList {
    rows: Vec<ReviewItem>,
    cursor: usize,
    window: ListWindow,
    /// The list is loading or a decision is running; the view takes no new one.
    busy: bool,
    editing: bool,
    form: Form,
    problem: Option<Problem>,
    filed: Option<FiledNotice>,
    /// What `begin_confirm` filed, until the work answers.
    pending_notice: Option<FiledNotice>,
    /// An entry to put the cursor on once the list has loaded, with the field
    /// to edit; set by the quick-fix menu.
    wanted: Option<(PathBuf, FormField)>,
    /// The field to focus after the load that found the wanted entry.
    focus_request: Option<FormField>,
    /// The pattern the user configured for documents no rule covers.
    custom_default_pattern: Option<String>,
    /// The pattern field holds what Hermes proposed, not something the user
    /// typed or a rule gave; it then follows the other fields.
    pattern_is_default: bool,
    tax_contexts: Vec<TaxContextEntry>,
    replan: Option<ReplanStep>,
    /// Tells the answer of the preview that is current from a stale one.
    replan_serial: u64,
    document_preview: DocumentPreview,
}

impl ReviewList {
    /// Opens busy: the list loads with the view.
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            cursor: 0,
            window: ListWindow::default(),
            busy: true,
            editing: false,
            form: Form::default(),
            problem: None,
            filed: None,
            pending_notice: None,
            wanted: None,
            focus_request: None,
            custom_default_pattern: None,
            pattern_is_default: false,
            tax_contexts: Vec::new(),
            replan: None,
            replan_serial: 0,
            document_preview: DocumentPreview::default(),
        }
    }

    /// The pattern proposed for documents without a rule or learned pattern,
    /// instead of the built-in ones.
    pub fn with_default_pattern(mut self, pattern: Option<String>) -> Self {
        self.custom_default_pattern = pattern;
        self
    }

    /// Where the proposed pattern of this entry comes from; `None` when no
    /// pattern fits.
    pub fn pattern_source(&self, item: &ReviewItem) -> Option<PatternSource> {
        choose_pattern(
            item.proposed_pattern.as_deref(),
            self.custom_default_pattern.as_deref(),
            &item.fields,
        )
        .map(|chosen| chosen.source)
    }

    /// Opens the form on the entry of `path` with the cursor in `field` as soon
    /// as the list has loaded.
    pub fn edit_on_load(&mut self, path: PathBuf, field: FormField) {
        self.wanted = Some((path, field));
    }

    /// The field to focus, once, after a load that found the wanted entry.
    pub fn take_focus_request(&mut self) -> Option<FormField> {
        self.focus_request.take()
    }

    pub fn rows(&self) -> &[ReviewItem] {
        &self.rows
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The first entry of the list that is shown.
    pub fn first_visible(&self) -> usize {
        self.window.offset(self.rows.len(), VISIBLE_ROWS)
    }

    pub fn selected(&self) -> Option<&ReviewItem> {
        self.rows.get(self.cursor)
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    /// The form has the keys.
    pub fn is_editing(&self) -> bool {
        self.editing
    }

    pub fn form(&self) -> &Form {
        &self.form
    }

    pub fn problem(&self) -> Option<&Problem> {
        self.problem.as_ref()
    }

    pub fn filed(&self) -> Option<&FiledNotice> {
        self.filed.as_ref()
    }

    pub fn document_preview(&self) -> &DocumentPreview {
        &self.document_preview
    }

    /// The request for the preview of the document under the cursor, if it
    /// is not the one the preview shows or waits for.
    pub fn preview_request(&mut self) -> Option<PreviewRequest> {
        match self.rows.get(self.cursor) {
            Some(item) => self.document_preview.request(&item.path, &item.passages),
            None => {
                self.document_preview.clear();
                None
            }
        }
    }

    pub fn preview_loaded(&mut self, serial: u64, content: PreviewContent) {
        self.document_preview.loaded(serial, content);
    }

    #[cfg(test)]
    pub fn show_preview_for_test(&mut self, content: PreviewContent) {
        self.document_preview = DocumentPreview::showing(content);
    }

    /// Open entries, for the badge of the sidebar; known once loaded.
    pub fn open_count(&self) -> Option<usize> {
        (!self.busy || !self.rows.is_empty()).then_some(self.rows.len())
    }

    /// Where the pattern files the document with the fields as typed, or
    /// `None` while they do not fit.
    pub fn preview(&self) -> Option<PathBuf> {
        let fields = self.form.fields().ok()?;
        render_pattern(self.form.pattern.trim(), &fields)
    }

    /// [`Self::preview`] in the pieces the path is made of, to show which come
    /// from which field.
    pub fn preview_parts(&self) -> Option<Vec<PatternPart>> {
        let fields = self.form.fields().ok()?;
        render_pattern_parts(self.form.pattern.trim(), &fields)
    }

    /// The user changed what the form holds for `field` since the document
    /// was loaded. Tags and the pattern have no origin to be changed from.
    pub fn is_edited(&self, field: Field) -> bool {
        let Some(item) = self.selected() else {
            return false;
        };
        let loaded = Form::of(item, self.custom_default_pattern.as_deref());
        match field {
            Field::Sender => self.form.sender != loaded.sender,
            Field::Kind => self.form.kind != loaded.kind,
            Field::TaxContext => self.form.tax_context != loaded.tax_context,
            Field::Date => self.form.date != loaded.date,
            Field::Number => self.form.number != loaded.number,
            Field::Amount => self.form.amount != loaded.amount,
        }
    }

    /// The field the pattern needs and the form leaves empty, if the pattern
    /// reads and the fields are valid.
    pub fn missing_field(&self) -> Option<Field> {
        let fields = self.form.fields().ok()?;
        missing_field(self.form.pattern.trim(), &fields)
    }

    /// The stored tax contexts the form offers, as of the last load.
    pub fn tax_contexts(&self) -> &[TaxContextEntry] {
        &self.tax_contexts
    }

    pub fn set_tax_contexts(&mut self, tax_contexts: Vec<TaxContextEntry>) {
        self.tax_contexts = tax_contexts;
    }

    /// The list the work returned. The cursor stays on the entry it was on, if
    /// that is still open.
    pub fn loaded(&mut self, rows: Vec<ReviewItem>) {
        let selected = self.selected().map(|row| row.entry);
        self.cursor = selected
            .and_then(|entry| rows.iter().position(|row| row.entry == entry))
            .unwrap_or_else(|| self.cursor.min(rows.len().saturating_sub(1)));
        self.rows = rows;
        self.window
            .ensure_visible(self.cursor, self.rows.len(), VISIBLE_ROWS);
        self.busy = false;
        self.editing = false;
        self.fill_form();
        self.go_to_wanted();
    }

    fn go_to_wanted(&mut self) {
        let Some((path, field)) = self.wanted.take() else {
            return;
        };
        let Some(position) = self.rows.iter().position(|row| row.path == path) else {
            return;
        };
        self.cursor = position;
        self.window
            .ensure_visible(position, self.rows.len(), VISIBLE_ROWS);
        self.fill_form();
        self.editing = true;
        self.focus_request = Some(field);
    }

    pub fn replan(&self) -> Option<&ReplanStep> {
        self.replan.as_ref()
    }

    /// Starts the preview step for `scope`; the number tells its answer from
    /// an older one. `None` while a step is up.
    pub fn begin_replan(&mut self, scope: ReplanScope) -> Option<u64> {
        if self.replan.is_some() {
            return None;
        }
        self.editing = false;
        self.problem = None;
        self.replan_serial = self.replan_serial.wrapping_add(1);
        self.replan = Some(ReplanStep::Looking(scope));
        Some(self.replan_serial)
    }

    /// The documents planned again. An answer for a step that was left, or
    /// for an older one, is dropped.
    pub fn previewed(&mut self, serial: u64, items: Vec<ReplanItem>) {
        if serial != self.replan_serial {
            return;
        }
        if let Some(ReplanStep::Looking(scope)) = self.replan.take() {
            self.replan = Some(ReplanStep::Preview { scope, items });
        }
    }

    /// Leaves the step; true when there was one.
    pub fn cancel_replan(&mut self) -> bool {
        self.replan.take().is_some()
    }

    /// How many of the previewed documents would be filed.
    pub fn replan_fileable(&self) -> usize {
        match &self.replan {
            Some(ReplanStep::Preview { items, .. }) => {
                items.iter().filter(|item| item.would_be_filed()).count()
            }
            Some(ReplanStep::Looking(_)) | None => 0,
        }
    }

    /// Ends the step with the user's confirm: the previewed documents and
    /// the scope they came from. `None` when nothing would be filed.
    pub fn confirm_replan(&mut self) -> Option<(ReplanScope, Vec<ReplanItem>)> {
        if self.replan_fileable() == 0 {
            return None;
        }
        match self.replan.take() {
            Some(ReplanStep::Preview { scope, items }) => Some((scope, items)),
            other => {
                self.replan = other;
                None
            }
        }
    }

    pub fn failed(&mut self, detail: String) {
        if matches!(self.replan, Some(ReplanStep::Looking(_))) {
            self.replan = None;
        }
        self.busy = false;
        self.pending_notice = None;
        self.problem = Some(Problem::Failed(detail));
    }

    pub fn move_cursor(&mut self, delta: isize) {
        let last = self.rows.len().saturating_sub(1);
        self.select(self.cursor.saturating_add_signed(delta).min(last));
    }

    pub fn select(&mut self, index: usize) {
        if self.busy || index >= self.rows.len() || index == self.cursor {
            return;
        }
        self.cursor = index;
        self.window
            .ensure_visible(index, self.rows.len(), VISIBLE_ROWS);
        self.editing = false;
        self.fill_form();
    }

    /// The wheel moves the window; the entry under the cursor follows it when
    /// the cursor would leave the window.
    pub fn scroll(&mut self, delta: ScrollDelta) {
        let mut cursor = self.cursor;
        self.window
            .scroll(delta, self.rows.len(), VISIBLE_ROWS, &mut cursor);
        self.select(cursor);
    }

    pub fn start_edit(&mut self) {
        if !self.busy && self.replan.is_none() && self.selected().is_some() {
            self.editing = true;
        }
    }

    pub fn cancel_edit(&mut self) {
        self.editing = false;
    }

    pub fn set_text(&mut self, field: FormField, text: String) {
        if self.selected().is_some() {
            *self.form.text_mut(field) = text;
            self.problem = None;
            if field == FormField::Pattern {
                self.pattern_is_default = false;
            } else if self.pattern_is_default {
                self.follow_fields_with_default_pattern();
            }
        }
    }

    fn follow_fields_with_default_pattern(&mut self) {
        if let Ok(fields) = self.form.fields() {
            self.form.pattern =
                default_pattern_for(self.custom_default_pattern.as_deref(), &fields)
                    .unwrap_or_default();
        }
    }

    pub fn set_tax_context(&mut self, tax_context: Option<TaxContext>) {
        if self.selected().is_some() {
            self.form.tax_context = tax_context;
            self.problem = None;
        }
    }

    /// The document to file and how: the fields and the pattern as shown. A
    /// typed value no field can hold, or a pattern that does not fit, is
    /// reported in the view and nothing is returned. Marks the view busy until
    /// the work answers.
    pub fn begin_confirm(&mut self) -> Option<ConfirmRequest> {
        if self.busy || self.replan.is_some() {
            return None;
        }
        let row = self.rows.get(self.cursor)?;
        let fields = match self.form.fields() {
            Ok(fields) => fields,
            Err(problem) => {
                self.problem = Some(problem);
                return None;
            }
        };
        let pattern = self.form.pattern.trim().to_owned();
        if render_pattern(&pattern, &fields).is_none() {
            self.problem = Some(Problem::PatternDoesNotFit);
            return None;
        }
        let request = ConfirmRequest {
            entry: row.entry,
            document_id: row.document_id,
            fields: fields.clone(),
            pattern: pattern.clone(),
        };
        self.pending_notice = Some(FiledNotice {
            path: PathBuf::new(),
            sender: fields.sender.unwrap_or_default(),
            kind: fields.kind.unwrap_or_default(),
            tax_context: fields.tax_context,
            tags: fields.tags,
            pattern,
        });
        self.busy = true;
        self.problem = None;
        self.filed = None;
        Some(request)
    }

    /// The answer of the filing. The list is loaded again after it, which ends
    /// the busy state.
    pub fn confirmed(&mut self, outcome: ConfirmOutcome) {
        let notice = self.pending_notice.take();
        match outcome {
            ConfirmOutcome::Filed { relative_path } => {
                self.filed = notice.map(|pending| FiledNotice {
                    path: relative_path,
                    ..pending
                });
            }
            ConfirmOutcome::TargetExists => self.problem = Some(Problem::TargetExists),
            ConfirmOutcome::Requeued => self.problem = Some(Problem::FileChanged),
        }
    }

    /// What the rule form starts with, for the offer after filing.
    pub fn rule_offer(&self) -> Option<RulePrefill> {
        if self.busy {
            return None;
        }
        self.filed
            .as_ref()
            .filter(|notice| notice.offers_rule())
            .map(FiledNotice::rule_prefill)
    }

    /// Takes the form from the entry under the cursor. What went wrong with
    /// the last try belongs to the document it was tried on.
    fn fill_form(&mut self) {
        let custom_default = self.custom_default_pattern.clone();
        self.form = self
            .selected()
            .map(|item| Form::of(item, custom_default.as_deref()))
            .unwrap_or_default();
        self.pattern_is_default = self
            .selected()
            .is_some_and(|item| item.proposed_pattern.is_none());
        self.problem = None;
    }
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
pub mod tests {
    use ncrs_index::{Index, ReviewReason};

    use super::*;
    use crate::review_work::TextState;

    /// Ids come from a real index: only the index hands them out.
    fn ids() -> Vec<(ReviewEntryId, DocumentId)> {
        ids_of(2)
    }

    fn ids_of(count: u8) -> Vec<(ReviewEntryId, DocumentId)> {
        use chrono::{TimeZone as _, Utc};
        use ncrs_index::{ContentHash, DocumentSource, NewDocument};

        let dir = tempfile::TempDir::new().unwrap();
        let mut index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        (1..=count)
            .map(|seed| {
                let document = index
                    .insert_document(&NewDocument {
                        hash: ContentHash([seed; 32]),
                        path: dir.path().join(format!("scan-{seed}.pdf")),
                        source: DocumentSource::Inbox,
                        received_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
                        stamp: None,
                    })
                    .unwrap();
                let entry = index
                    .add_review(document, ReviewReason::NoMatchingRule)
                    .unwrap();
                (entry, document)
            })
            .collect()
    }

    pub fn item(
        (entry, document_id): (ReviewEntryId, DocumentId),
        name: &str,
        sender: &str,
    ) -> ReviewItem {
        ReviewItem {
            entry,
            document_id,
            path: PathBuf::from(format!("inbox/{name}")),
            reason: ReviewReason::NoMatchingRule,
            text_state: TextState::Read,
            fields: DocumentFields {
                sender: (!sender.is_empty()).then(|| sender.to_owned()),
                kind: Some("invoice".to_owned()),
                tax_context: Some(TaxContext::named("company")),
                date: chrono::NaiveDate::from_ymd_opt(2024, 3, 9),
                ..DocumentFields::default()
            },
            proposed_pattern: Some("{sender}/{date}_{kind}".to_owned()),
            tax_context_assigned_by: None,
            origins: ncrs_index::FieldOrigins::default(),
            passages: Vec::new(),
        }
    }

    pub fn stored_tax_contexts() -> Vec<TaxContextEntry> {
        let dir = tempfile::TempDir::new().unwrap();
        let index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        index.tax_contexts().unwrap()
    }

    pub fn loaded_list() -> ReviewList {
        let [one, two] = <[_; 2]>::try_from(ids()).unwrap();
        let mut list = ReviewList::new();
        list.set_tax_contexts(stored_tax_contexts());
        list.loaded(vec![
            item(one, "scan-1.pdf", "Company A"),
            item(two, "scan-2.pdf", "Agency B"),
        ]);
        list
    }

    /// Two documents no rule covers: one the default pattern fills in, one
    /// without a date that it cannot.
    pub fn default_pattern_list() -> ReviewList {
        let [one, two] = <[_; 2]>::try_from(ids()).unwrap();
        let mut frost = item(one, "scan-1.pdf", "Frost");
        frost.proposed_pattern = None;
        frost.fields.number = Some("3318".to_owned());
        let mut undated = item(two, "scan-2.pdf", "Agency B");
        undated.proposed_pattern = None;
        undated.fields.date = None;
        let mut list = ReviewList::new();
        list.set_tax_contexts(stored_tax_contexts());
        list.loaded(vec![frost, undated]);
        list
    }

    /// One document whose text gave nothing.
    pub fn nothing_recognised_list() -> ReviewList {
        let [one, _] = <[_; 2]>::try_from(ids()).unwrap();
        let mut blank = item(one, "scan-1.pdf", "");
        blank.proposed_pattern = None;
        blank.fields = DocumentFields::default();
        let mut list = ReviewList::new();
        list.set_tax_contexts(stored_tax_contexts());
        list.loaded(vec![blank]);
        list
    }

    /// A document a condition assigned a context to, one no condition fits and
    /// one that several conditions fit.
    pub fn tax_context_list() -> ReviewList {
        use ncrs_index::ConditionKind;
        use ncrs_intake::AssigningCondition;

        let [first, second, third] = <[_; 3]>::try_from(ids_of(3)).unwrap();
        let mut assigned = item(first, "scan-1.pdf", "Telekom");
        assigned.tax_context_assigned_by = Some(AssigningCondition {
            kind: ConditionKind::Recipient,
            value: "Firma A GmbH".to_owned(),
        });
        assigned.origins = ncrs_index::FieldOrigins::default().with(
            Field::TaxContext,
            ncrs_index::FieldOrigin::Condition {
                kind: ConditionKind::Recipient,
                value: "Firma A GmbH".to_owned(),
            },
        );
        let mut none_fit = item(second, "scan-2.pdf", "Agency B");
        none_fit.reason = ReviewReason::NoTaxContextMatches;
        none_fit.fields.tax_context = None;
        let mut several_fit = item(third, "scan-3.pdf", "Frost");
        several_fit.reason = ReviewReason::SeveralTaxContextsMatch;
        several_fit.fields.tax_context = None;
        let mut list = ReviewList::new();
        list.set_tax_contexts(stored_tax_contexts());
        list.loaded(vec![assigned, none_fit, several_fit]);
        list
    }

    /// One document whose fields come from a rule, the AI, the user and the
    /// condition of a tax context, with the passages of its text that led there.
    pub fn origins_list() -> ReviewList {
        use ncrs_index::{ConditionKind, FieldOrigin, FieldOrigins};

        let [one, _] = <[_; 2]>::try_from(ids()).unwrap();
        let mut origin_item = item(one, "scan-1.pdf", "Company A");
        origin_item.fields.number = Some("4711".to_owned());
        origin_item.origins = FieldOrigins::default()
            .with(Field::Sender, FieldOrigin::Rule(None))
            .with(
                Field::Kind,
                FieldOrigin::Ai {
                    confidence_percent: 82,
                },
            )
            .with(Field::Number, FieldOrigin::Manual)
            .with(Field::Date, FieldOrigin::Rule(None))
            .with(
                Field::TaxContext,
                FieldOrigin::Condition {
                    kind: ConditionKind::Recipient,
                    value: "Firma A GmbH".to_owned(),
                },
            );
        origin_item.passages = vec!["Company A".to_owned(), "4711".to_owned()];
        let mut list = ReviewList::new();
        list.set_tax_contexts(stored_tax_contexts());
        list.loaded(vec![origin_item]);
        list
    }

    /// More entries than the list shows, the first with names too long for
    /// the file column and the second with a short one.
    pub fn long_list() -> ReviewList {
        const LONG: &str =
            "2024-03-09_Stadtwerke_Musterhausen_Jahresabrechnung_Strom_und_Gas_Kundennummer_4711.pdf";
        let mut list = ReviewList::new();
        list.set_tax_contexts(stored_tax_contexts());
        list.loaded(
            ids_of(200)
                .into_iter()
                .enumerate()
                .map(|(position, entry)| match position {
                    0 => item(
                        entry,
                        LONG,
                        "Stadtwerke Musterhausen Energie und Wasser GmbH",
                    ),
                    1 => item(entry, "a.pdf", "A"),
                    _ => item(entry, &format!("scan-{position}.pdf"), "Agency B"),
                })
                .collect(),
        );
        list
    }

    fn replan_item(row: &ReviewItem, after: crate::replan_work::ReplanAfter) -> ReplanItem {
        ReplanItem {
            document_id: row.document_id,
            review: Some(row.entry),
            path: row.path.clone(),
            now: Some(row.reason),
            after,
        }
    }

    #[test]
    fn replan_the_step_files_nothing_by_hand_and_an_old_answer_is_dropped() {
        use crate::replan_work::ReplanAfter;

        let mut list = loaded_list();
        let first = list.rows()[0].clone();
        let serial = list.begin_replan(ReplanScope::OpenCases).unwrap();
        assert!(list.begin_replan(ReplanScope::OpenCases).is_none());
        assert!(list.begin_confirm().is_none(), "Enter does not file a row");
        assert!(list.confirm_replan().is_none(), "nothing is previewed yet");

        list.previewed(serial.wrapping_add(1), Vec::new());
        assert!(matches!(list.replan(), Some(ReplanStep::Looking(_))));

        let path = PathBuf::from("Company A/invoice.pdf");
        list.previewed(serial, vec![replan_item(&first, ReplanAfter::Filed(path))]);
        assert_eq!(list.replan_fileable(), 1);
        let (scope, items) = list.confirm_replan().unwrap();
        assert_eq!(scope, ReplanScope::OpenCases);
        assert_eq!(items.len(), 1);
        assert!(list.replan().is_none());
    }

    #[test]
    fn replan_a_preview_without_a_document_to_file_cannot_be_confirmed() {
        use crate::replan_work::ReplanAfter;

        let mut list = loaded_list();
        let first = list.rows()[0].clone();
        let serial = list.begin_replan(ReplanScope::OpenCases).unwrap();
        list.previewed(serial, vec![replan_item(&first, ReplanAfter::Unread)]);

        assert_eq!(list.replan_fileable(), 0);
        assert!(list.confirm_replan().is_none());
        assert!(list.replan().is_some(), "the step stays until it is left");
        assert!(list.cancel_replan());
        assert!(!list.cancel_replan());
    }

    #[test]
    fn without_a_rule_the_form_starts_with_the_default_pattern_and_files_nothing() {
        let mut list = default_pattern_list();

        assert_eq!(
            list.form().pattern,
            "{year}/{year}{month}{day}_{sender}_{number}"
        );
        assert_eq!(
            list.pattern_source(&list.rows()[0]),
            Some(PatternSource::Default)
        );
        assert_eq!(
            list.preview(),
            Some(PathBuf::from("2024/20240309_Frost_3318"))
        );
        assert!(list.filed().is_none());

        list.select(1);
        assert_eq!(list.form().pattern, "", "no date, no pattern");
        assert_eq!(list.pattern_source(&list.rows()[1]), None);
    }

    #[test]
    fn a_rule_pattern_is_kept_over_the_default() {
        let list = loaded_list();

        assert_eq!(list.form().pattern, "{sender}/{date}_{kind}");
        assert_eq!(
            list.pattern_source(&list.rows()[0]),
            Some(PatternSource::RuleOrProfile)
        );
    }

    #[test]
    fn a_configured_default_replaces_the_built_in_ones() {
        let [one, _] = <[_; 2]>::try_from(ids()).unwrap();
        let mut item = item(one, "scan-1.pdf", "Frost");
        item.proposed_pattern = None;
        let mut list = ReviewList::new().with_default_pattern(Some("{year}/{kind}".to_owned()));
        list.loaded(vec![item]);

        assert_eq!(list.form().pattern, "{year}/{kind}");
    }

    #[test]
    fn the_default_pattern_follows_the_fields_until_the_pattern_is_typed() {
        let mut list = default_pattern_list();
        list.select(1);

        list.set_text(FormField::Date, "2024-03-09".to_owned());
        assert_eq!(list.form().pattern, "{year}/{year}{month}{day}_{sender}");

        list.set_text(FormField::Pattern, "{kind}".to_owned());
        list.set_text(FormField::Number, "1".to_owned());
        assert_eq!(list.form().pattern, "{kind}");
    }

    #[test]
    fn a_field_counts_as_edited_once_the_form_differs_from_what_was_loaded() {
        let mut list = loaded_list();
        assert!(!list.is_edited(Field::Sender));
        list.start_edit();
        list.set_text(FormField::Sender, "Company AX".to_owned());
        assert!(list.is_edited(Field::Sender));
        assert!(!list.is_edited(Field::Kind));
        list.set_text(FormField::Sender, "Company A".to_owned());
        assert!(!list.is_edited(Field::Sender));
    }

    #[test]
    fn the_preview_names_the_field_a_pattern_misses() {
        let mut list = loaded_list();
        list.set_text(FormField::Sender, String::new());

        assert_eq!(list.preview(), None);
        assert_eq!(list.missing_field(), Some(Field::Sender));
    }

    #[test]
    fn a_wanted_entry_gets_the_cursor_and_the_form_once_the_list_has_loaded() {
        let rows = loaded_list().rows().to_vec();
        let mut reopened = ReviewList::new();
        reopened.edit_on_load(rows[1].path.clone(), FormField::Kind);
        reopened.loaded(rows);
        assert_eq!(reopened.cursor(), 1);
        assert!(reopened.is_editing());
        assert_eq!(reopened.take_focus_request(), Some(FormField::Kind));
        assert_eq!(reopened.take_focus_request(), None);
    }

    #[test]
    fn the_list_opens_busy_and_empty() {
        let list = ReviewList::new();

        assert!(list.is_busy());
        assert!(list.rows().is_empty());
        assert_eq!(list.open_count(), None);
    }

    #[test]
    fn loading_fills_the_form_from_the_first_entry() {
        let list = loaded_list();

        assert!(!list.is_busy());
        assert_eq!(list.open_count(), Some(2));
        assert_eq!(list.form().sender, "Company A");
        assert_eq!(list.form().date, "2024-03-09");
        assert_eq!(list.form().pattern, "{sender}/{date}_{kind}");
        assert_eq!(
            list.preview(),
            Some(PathBuf::from("Company A/2024-03-09_invoice"))
        );
    }

    #[test]
    fn moving_the_cursor_loads_the_form_of_the_other_entry() {
        let mut list = loaded_list();
        list.set_text(FormField::Sender, "Changed".to_owned());

        list.move_cursor(1);

        assert_eq!(list.cursor(), 1);
        assert_eq!(list.form().sender, "Agency B");
        list.move_cursor(-1);
        assert_eq!(list.form().sender, "Company A");
        list.move_cursor(-1);
        assert_eq!(list.cursor(), 0);
    }

    #[test]
    fn a_value_no_field_can_hold_is_a_problem_and_nothing_is_filed() {
        let mut list = loaded_list();

        list.set_text(FormField::Date, "9.3.2024".to_owned());
        assert_eq!(list.begin_confirm(), None);
        assert_eq!(list.problem(), Some(&Problem::InvalidDate));

        list.set_text(FormField::Date, "2024-03-09".to_owned());
        list.set_text(FormField::Amount, "lots".to_owned());
        assert_eq!(list.begin_confirm(), None);
        assert_eq!(list.problem(), Some(&Problem::InvalidAmount));
        assert!(!list.is_busy());
    }

    #[test]
    fn a_pattern_that_needs_an_empty_field_does_not_fit() {
        let mut list = loaded_list();
        list.set_text(FormField::Sender, " ".to_owned());

        assert_eq!(list.begin_confirm(), None);
        assert_eq!(list.problem(), Some(&Problem::PatternDoesNotFit));

        list.set_text(FormField::Sender, "Company A".to_owned());
        list.set_text(FormField::Pattern, "{nonsense}".to_owned());
        assert_eq!(list.begin_confirm(), None);
        assert_eq!(list.preview(), None);
    }

    #[test]
    fn confirming_hands_over_the_edited_fields_and_marks_the_list_busy() {
        let mut list = loaded_list();
        list.set_text(FormField::Sender, " Company C ".to_owned());
        list.set_text(FormField::Tags, "tax, travel".to_owned());
        list.set_tax_context(Some(TaxContext::named("private")));

        let request = list.begin_confirm().unwrap();

        assert_eq!(request.fields.sender.as_deref(), Some("Company C"));
        assert_eq!(
            request.fields.tax_context,
            Some(TaxContext::named("private"))
        );
        assert_eq!(request.fields.tags.len(), 2);
        assert_eq!(request.pattern, "{sender}/{date}_{kind}");
        assert!(list.is_busy());
        assert_eq!(list.begin_confirm(), None);
    }

    #[test]
    fn a_filed_document_offers_a_rule_for_its_sender() {
        let mut list = loaded_list();
        list.set_text(FormField::Tags, "tax, travel".to_owned());
        list.set_tax_context(Some(TaxContext::named("private")));
        list.begin_confirm().unwrap();
        list.confirmed(ConfirmOutcome::Filed {
            relative_path: PathBuf::from("Company A/2024-03-09_invoice.pdf"),
        });
        list.loaded(vec![list.rows()[1].clone()]);

        let filed = list.filed().unwrap();
        assert_eq!(
            filed.path,
            PathBuf::from("Company A/2024-03-09_invoice.pdf")
        );
        assert!(filed.offers_rule());
        let offer = list.rule_offer().unwrap();
        assert_eq!(offer.sender, "Company A");
        assert_eq!(offer.kind, "invoice");
        assert_eq!(offer.tax_context, Some(TaxContext::named("private")));
        assert_eq!(
            offer.tags,
            BTreeSet::from(["tax".to_owned(), "travel".to_owned()])
        );
        assert_eq!(offer.pattern, "{sender}/{date}_{kind}");
    }

    #[test]
    fn no_rule_is_offered_for_a_document_without_a_sender() {
        let mut list = loaded_list();
        list.set_text(FormField::Sender, String::new());
        list.set_text(FormField::Pattern, "{kind}/{date}".to_owned());

        list.begin_confirm().unwrap();
        list.confirmed(ConfirmOutcome::Filed {
            relative_path: PathBuf::from("invoice/2024-03-09.pdf"),
        });

        assert!(!list.filed().unwrap().offers_rule());
    }

    #[test]
    fn a_taken_place_is_reported_and_the_entry_stays() {
        let mut list = loaded_list();
        list.begin_confirm().unwrap();

        list.confirmed(ConfirmOutcome::TargetExists);
        list.loaded(list.rows().to_vec());

        assert_eq!(list.rows().len(), 2);
        assert_eq!(list.filed(), None);
    }

    #[test]
    fn a_failure_ends_busy_and_is_shown() {
        let mut list = loaded_list();
        list.begin_confirm().unwrap();

        list.failed("index not open".to_owned());

        assert!(!list.is_busy());
        assert_eq!(
            list.problem(),
            Some(&Problem::Failed("index not open".to_owned()))
        );
    }

    #[test]
    fn an_empty_list_has_nothing_to_edit_or_file() {
        let mut list = ReviewList::new();
        list.set_tax_contexts(stored_tax_contexts());
        list.loaded(Vec::new());

        list.start_edit();

        assert!(!list.is_editing());
        assert_eq!(list.begin_confirm(), None);
        assert_eq!(list.preview(), None);
        assert_eq!(list.open_count(), Some(0));
    }
}
