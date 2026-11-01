//! State of the rules page: the confirmed rules in their order, the cursor in
//! them, the delete question, and the sentences the rules are shown as. No
//! widgets and no tasks; `App` owns it and changes it from `update`, the view
//! is `ui::rules`.

use iced::mouse::ScrollDelta;

use std::sync::Arc;

use ncrs_index::{Combinator, Condition, ConditionKind, KindCatalog, RuleId, RuleOrigin, RuleSpec};

use crate::filing_pieces::FilingTarget;
use crate::message::HermesPage;
use crate::rule_form::{RuleForm, RuleFormTarget, RuleSave};
use ncrs_filing::rules_work::{ListedRule, RuleChange, RulesLoad};
use ncrs_i18n::fill_template;
use ncrs_i18n::{Language, Msg};
use ncrs_ui_kit::list_window::ListWindow;

/// Most rules of the list shown at once; the list follows the cursor.
pub const VISIBLE_ROWS: usize = 12;

#[derive(Debug, Clone, PartialEq)]
pub struct RulesPage {
    rows: Vec<ListedRule>,
    cursor: usize,
    window: ListWindow,
    /// The list is loading or a change is running; the page takes no new one.
    busy: bool,
    confirming_delete: bool,
    /// The work on the index failed; the technical detail stays English.
    problem: Option<String>,
    /// The form of the selected rule, or of a rule that is not listed yet.
    form: Option<RuleForm>,
    /// The keys go to the fields of the form, not to the list.
    editing: bool,
    /// The documents of the try fill the detail instead of the form.
    try_list_open: bool,
    /// The page the form was opened from; the card goes back to it when the
    /// form is saved or cancelled.
    return_to: Option<HermesPage>,
    /// The rule the first load puts the cursor on.
    select_on_load: Option<RuleId>,
    kinds: Arc<KindCatalog>,
}

impl Default for RulesPage {
    fn default() -> Self {
        Self::new()
    }
}

impl RulesPage {
    /// Opens busy: the list loads with the view.
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            cursor: 0,
            window: ListWindow::default(),
            busy: true,
            confirming_delete: false,
            problem: None,
            form: None,
            editing: false,
            try_list_open: false,
            return_to: None,
            select_on_load: None,
            kinds: Arc::default(),
        }
    }

    pub fn with_kinds(self, kinds: Arc<KindCatalog>) -> Self {
        Self { kinds, ..self }
    }

    pub fn kinds(&self) -> &KindCatalog {
        &self.kinds
    }

    /// The next load puts the cursor on this rule, if it is listed.
    pub fn select_on_load(&mut self, id: RuleId) {
        self.select_on_load = Some(id);
    }

    pub fn rows(&self) -> &[ListedRule] {
        &self.rows
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The first rule of the list that is shown.
    pub fn first_visible(&self) -> usize {
        self.window.offset(self.rows.len(), VISIBLE_ROWS)
    }

    pub fn selected(&self) -> Option<&ListedRule> {
        self.rows.get(self.cursor)
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    pub fn is_confirming_delete(&self) -> bool {
        self.confirming_delete
    }

    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    pub fn form(&self) -> Option<&RuleForm> {
        self.form.as_ref()
    }

    pub fn form_mut(&mut self) -> Option<&mut RuleForm> {
        self.form.as_mut()
    }

    pub fn is_editing(&self) -> bool {
        self.editing
    }

    /// The keys go to the fields.
    pub fn start_editing(&mut self) {
        self.editing = self.form.is_some();
    }

    /// The keys go to the list again; whether they went to the fields.
    pub fn stop_editing(&mut self) -> bool {
        std::mem::take(&mut self.editing)
    }

    pub fn is_try_list_open(&self) -> bool {
        self.try_list_open
    }

    pub fn toggle_try_list(&mut self) {
        self.try_list_open = self.form.is_some() && !self.try_list_open;
    }

    /// Whether the form is for a rule that is not in the list.
    pub fn has_unlisted_form(&self) -> bool {
        self.form.as_ref().is_some_and(|form| {
            matches!(
                form.target,
                RuleFormTarget::New
                    | RuleFormTarget::FromDocument
                    | RuleFormTarget::FromSuggestion(_)
            )
        })
    }

    /// The form holds changes that are neither saved nor cancelled.
    pub fn has_unsaved_changes(&self) -> bool {
        self.form.as_ref().is_some_and(RuleForm::is_edited)
    }

    /// The list takes no change while a change runs or while the form holds
    /// unsaved changes: another rule, a new rule or a reload would drop them.
    fn list_is_locked(&self) -> bool {
        self.busy || self.has_unsaved_changes()
    }

    /// Shows the form of the rule under the cursor and drops what was typed.
    fn show_selected(&mut self) {
        self.return_to = None;
        self.editing = false;
        self.try_list_open = false;
        self.form = self
            .rows
            .get(self.cursor)
            .map(|listed| RuleForm::edit(&listed.rule));
    }

    /// Opens the form for a new rule.
    pub fn begin_new(&mut self) {
        if self.list_is_locked() {
            return;
        }
        self.confirming_delete = false;
        self.problem = None;
        self.return_to = None;
        self.try_list_open = false;
        self.editing = true;
        self.form = Some(RuleForm::new());
    }

    /// Opens a form made elsewhere, also while the list still loads.
    pub fn open_form(&mut self, form: RuleForm, return_to: Option<HermesPage>) {
        self.confirming_delete = false;
        self.problem = None;
        self.return_to = return_to;
        self.try_list_open = false;
        self.editing = true;
        self.form = Some(form);
    }

    /// Enter in the list: the keys go to the fields of the selected rule.
    pub fn begin_edit(&mut self) {
        if self.busy {
            return;
        }
        if self.form.is_none() {
            self.show_selected();
        }
        self.start_editing();
    }

    /// Drops what was typed and shows the selected rule again; whether there
    /// was a form.
    pub fn cancel_form(&mut self) -> bool {
        let had_form = self.form.is_some();
        let back = self.return_to.take();
        self.show_selected();
        self.return_to = back;
        had_form
    }

    /// The write the form asks for, once and only while it is valid.
    pub fn begin_save(&mut self) -> Option<RuleSave> {
        self.form.as_mut()?.save_request()
    }

    /// The write is done: the form closes. The caller loads the list again,
    /// or goes back to the page the form came from.
    pub fn saved(&mut self, id: RuleId) -> Option<HermesPage> {
        self.select_on_load = Some(id);
        self.form = None;
        self.editing = false;
        self.try_list_open = false;
        self.busy = true;
        self.return_to.take()
    }

    /// The page to go back to after the form was cancelled.
    pub fn take_return_to(&mut self) -> Option<HermesPage> {
        self.return_to.take()
    }

    pub fn save_failed(&mut self, detail: String) {
        if let Some(form) = self.form.as_mut() {
            form.save_failed(detail);
        }
    }

    /// The list the work returned. The cursor stays on the rule it was on, if
    /// that is still there.
    pub fn loaded(&mut self, load: RulesLoad) {
        let selected = self
            .select_on_load
            .take()
            .or_else(|| self.selected().map(|row| row.rule.id));
        self.cursor = selected
            .and_then(|id| load.rules.iter().position(|row| row.rule.id == id))
            .unwrap_or_else(|| self.cursor.min(load.rules.len().saturating_sub(1)));
        self.rows = load.rules;
        self.window
            .ensure_visible(self.cursor, self.rows.len(), VISIBLE_ROWS);
        if !self.has_unlisted_form() {
            self.show_selected();
        }
        self.busy = false;
        self.confirming_delete = false;
        self.problem = None;
    }

    pub fn failed(&mut self, detail: String) {
        self.busy = false;
        self.confirming_delete = false;
        self.problem = Some(detail);
    }

    pub fn move_cursor(&mut self, delta: isize) {
        let last = self.rows.len().saturating_sub(1);
        self.select(self.cursor.saturating_add_signed(delta).min(last));
    }

    /// The wheel moves the window; the cursor follows it when it would leave.
    pub fn scroll(&mut self, delta: ScrollDelta) {
        let mut cursor = self.cursor;
        self.window
            .scroll(delta, self.rows.len(), VISIBLE_ROWS, &mut cursor);
        self.select(cursor);
    }

    /// Selecting another rule drops what was typed into the form of the one
    /// before; so does selecting the rule of a form that is not listed yet.
    pub fn select(&mut self, index: usize) {
        if self.list_is_locked() || index >= self.rows.len() {
            return;
        }
        if index == self.cursor && !self.has_unlisted_form() {
            return;
        }
        self.cursor = index;
        self.window
            .ensure_visible(index, self.rows.len(), VISIBLE_ROWS);
        self.confirming_delete = false;
        self.problem = None;
        self.show_selected();
    }

    /// Flips the switch of the rule at `index` and returns the change to make.
    pub fn begin_toggle(&mut self, index: usize) -> Option<RuleChange> {
        if self.list_is_locked() {
            return None;
        }
        self.select(index);
        let row = self.rows.get(index)?;
        let change = RuleChange::SetEnabled {
            id: row.rule.id,
            enabled: !row.rule.spec.enabled,
        };
        self.begin(change)
    }

    /// Moves the selected rule one place among the listed ones. The target is
    /// the place of the neighbour among all rules, suggestions included.
    pub fn begin_move(&mut self, delta: isize) -> Option<RuleChange> {
        if self.list_is_locked() {
            return None;
        }
        let neighbour = self.rows.get(self.cursor.checked_add_signed(delta)?)?;
        let id = self.selected()?.rule.id;
        self.begin(RuleChange::Move {
            id,
            to_index: neighbour.global_index,
        })
    }

    pub fn ask_delete(&mut self) {
        if !self.list_is_locked() && self.selected().is_some() {
            self.confirming_delete = true;
        }
    }

    pub fn begin_delete(&mut self) -> Option<RuleChange> {
        if !self.confirming_delete {
            return None;
        }
        let id = self.selected()?.rule.id;
        self.begin(RuleChange::Delete { id })
    }

    /// Backs out of the delete question; whether there was one.
    pub fn cancel_delete(&mut self) -> bool {
        std::mem::take(&mut self.confirming_delete)
    }

    fn begin(&mut self, change: RuleChange) -> Option<RuleChange> {
        self.busy = true;
        self.problem = None;
        Some(change)
    }
}

fn condition_template(kind: ConditionKind, negated: bool) -> Msg {
    match (kind, negated) {
        (ConditionKind::Substring, false) => Msg::RulesSentenceSubstring,
        (ConditionKind::Substring, true) => Msg::RulesSentenceSubstringNot,
        (ConditionKind::Word, false) => Msg::RulesSentenceWord,
        (ConditionKind::Word, true) => Msg::RulesSentenceWordNot,
        (ConditionKind::Regex, false) => Msg::RulesSentenceRegex,
        (ConditionKind::Regex, true) => Msg::RulesSentenceRegexNot,
        (ConditionKind::Sender, false) => Msg::RulesSentenceSender,
        (ConditionKind::Sender, true) => Msg::RulesSentenceSenderNot,
        (ConditionKind::SenderAddress, false) => Msg::RulesSentenceSenderAddress,
        (ConditionKind::SenderAddress, true) => Msg::RulesSentenceSenderAddressNot,
        (ConditionKind::Iban, false) => Msg::RulesSentenceIban,
        (ConditionKind::Iban, true) => Msg::RulesSentenceIbanNot,
        (ConditionKind::TaxId, false) => Msg::RulesSentenceTaxId,
        (ConditionKind::TaxId, true) => Msg::RulesSentenceTaxIdNot,
        (ConditionKind::Recipient, false) => Msg::RulesSentenceRecipient,
        (ConditionKind::Recipient, true) => Msg::RulesSentenceRecipientNot,
        (ConditionKind::Folder, false) => Msg::RulesSentenceFolder,
        (ConditionKind::Folder, true) => Msg::RulesSentenceFolderNot,
        (ConditionKind::Tag, false) => Msg::RulesSentenceTag,
        (ConditionKind::Tag, true) => Msg::RulesSentenceTagNot,
    }
}

fn condition_sentence(condition: &Condition, lang: Language) -> String {
    fill_template(
        lang.text(condition_template(condition.kind, condition.negated)),
        &[("{value}", &condition.value)],
    )
}

/// One condition in the sentence; a tag reads as the bare tag.
pub fn sentence_part(condition: &Condition, first: bool, lang: Language) -> (bool, String) {
    let bare_tag = condition.kind == ConditionKind::Tag && (!condition.negated || !first);
    if bare_tag {
        (condition.negated, condition.value.clone())
    } else {
        (false, condition_sentence(condition, lang))
    }
}

/// The rule as one sentence: the conditions, then after the arrow the kind,
/// the tags to set and where the document moves to.
pub fn rule_sentence(spec: &RuleSpec, lang: Language, kinds: &KindCatalog) -> String {
    let combinator = spec.conditions.combinator;
    let mut conditions = String::new();
    for (position, condition) in spec.conditions.list.iter().enumerate() {
        let (not_joiner, text) = sentence_part(condition, position == 0, lang);
        if position > 0 {
            conditions.push(' ');
            conditions.push_str(lang.text(match (combinator, not_joiner) {
                (Combinator::AllOf, false) => Msg::RulesJoinerAll,
                (Combinator::AnyOf, false) => Msg::RulesJoinerAny,
                (Combinator::AllOf, true) => Msg::RulesJoinerAllNot,
                (Combinator::AnyOf, true) => Msg::RulesJoinerAnyNot,
            }));
            conditions.push(' ');
        }
        conditions.push_str(&text);
    }
    let mut target = Vec::new();
    if let Some(kind) = &spec.kind {
        target.push(lang.kind_label(kinds, kind).into_owned());
    }
    if !spec.tags.is_empty() {
        target.push(
            spec.tags
                .iter()
                .map(|tag| fill_template(lang.text(Msg::RulesSentenceSetTag), &[("{tag}", tag)]))
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    target.push(FilingTarget::parse(&spec.filing_pattern).in_words(lang));
    fill_template(
        lang.text(Msg::RulesRuleIf),
        &[
            ("{conditions}", &conditions),
            ("{target}", &target.join(" · ")),
        ],
    )
}

pub fn origin_text(origin: RuleOrigin, lang: Language) -> &'static str {
    lang.text(match origin {
        RuleOrigin::Hand => Msg::RulesOriginHand,
        RuleOrigin::Suggestion => Msg::RulesOriginSuggestion,
        RuleOrigin::Filing => Msg::RulesOriginFiling,
    })
}

pub fn hits_text(hits: u64, lang: Language) -> String {
    if hits == 1 {
        lang.text(Msg::RulesHitsOne).to_owned()
    } else {
        fill_template(
            lang.text(Msg::RulesHitsOther),
            &[("{count}", &ncrs_ui_kit::format::grouped(hits))],
        )
    }
}

// Shared with the tests of the app and of other pages.
// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(any(test, feature = "test-support"))]
pub mod fixtures;

#[cfg(test)]
// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests;
