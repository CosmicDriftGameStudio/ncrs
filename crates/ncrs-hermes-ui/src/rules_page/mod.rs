//! State of the rules page: the confirmed rules in their order, the rule the
//! last action was on, the delete question, and the sentences the rules are
//! shown as. No widgets and no tasks; the Hermes UI state owns it and changes it from
//! `update`, the view is `ui::rules`.

use std::collections::BTreeSet;
use std::sync::Arc;

use ncrs_index::{
    Combinator, Condition, ConditionKind, KindCatalog, RuleId, RuleOrigin, RuleSpec, TargetId,
};

use crate::filing_pieces::FilingTarget;
use crate::rule_form::{RuleForm, RuleFormTarget, RuleSave};
use ncrs_filing::rules_work::{ListedRule, RuleChange, RulesLoad};
use ncrs_filing::sorting::TargetState;
use ncrs_i18n::fill_template;
use ncrs_i18n::{Language, Msg};
use ncrs_ui_kit::kit::condition_kind_name;

#[derive(Debug, Clone, PartialEq)]
pub struct RulesPage {
    rows: Vec<ListedRule>,
    /// The rule the last action was on; the delete question is about it.
    cursor: usize,
    /// The list is loading or a change is running; the page takes no new one.
    busy: bool,
    confirming_delete: bool,
    /// The work on the index failed; the technical detail stays English.
    problem: Option<String>,
    /// The form of the dialog: a rule of the list, or one that is not listed
    /// yet. No form, no dialog.
    form: Option<RuleForm>,
    /// The rule the next load puts the cursor on.
    select_on_load: Option<RuleId>,
    kinds: Arc<KindCatalog>,
    /// Made only to hold the form: opened from a panel or a plan, with no
    /// page behind it. The list is not shown and nothing loads.
    form_only: bool,
    /// The targets the user folded; the others show their rules.
    folded: BTreeSet<TargetId>,
    target_states: Vec<(TargetId, TargetState)>,
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
            busy: true,
            confirming_delete: false,
            problem: None,
            form: None,
            select_on_load: None,
            kinds: Arc::default(),
            form_only: false,
            folded: BTreeSet::new(),
            target_states: Vec::new(),
        }
    }

    /// A page that only holds `form`: not busy, the list stays empty.
    pub fn for_form(form: RuleForm, kinds: Arc<KindCatalog>) -> Self {
        Self {
            busy: false,
            form: Some(form),
            form_only: true,
            kinds,
            ..Self::new()
        }
    }

    pub fn is_form_only(&self) -> bool {
        self.form_only
    }

    /// The rules of one target with their place in the whole list, in the
    /// order they apply.
    pub fn rows_of<'a>(
        &'a self,
        target: &'a TargetId,
    ) -> impl Iterator<Item = (usize, &'a ListedRule)> + 'a {
        self.rows
            .iter()
            .enumerate()
            .filter(move |(_, row)| row.rule.target == *target)
    }

    /// Targets start unfolded; only the ones the user folded are remembered.
    pub fn is_unfolded(&self, target: &TargetId) -> bool {
        !self.folded.contains(target)
    }

    /// Folds or unfolds the rules of a target.
    pub fn toggle_fold(&mut self, target: TargetId) {
        if !self.folded.remove(&target) {
            self.folded.insert(target);
        }
    }

    pub fn set_target_states(&mut self, states: Vec<(TargetId, TargetState)>) {
        self.target_states = states;
    }

    /// What the disk said of the target; `None` until it has said it.
    pub fn target_state(&self, target: &TargetId) -> Option<TargetState> {
        self.target_states
            .iter()
            .find(|(id, _)| id == target)
            .map(|(_, state)| *state)
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

    /// A dialog is up: the form or the delete question.
    pub fn dialog_is_open(&self) -> bool {
        self.form.is_some() || self.confirming_delete
    }

    /// The list takes no change while a change runs.
    fn list_is_locked(&self) -> bool {
        self.busy
    }

    /// Opens the form for a new rule of the main target.
    pub fn begin_new(&mut self) {
        self.begin_new_in(TargetId::main());
    }

    /// Opens the form for a new rule of `target`.
    pub fn begin_new_in(&mut self, target: TargetId) {
        if self.list_is_locked() {
            return;
        }
        self.confirming_delete = false;
        self.problem = None;
        self.form = Some(RuleForm::new_in(target));
    }

    /// Opens a form made elsewhere, also while the list still loads.
    pub fn open_form(&mut self, form: RuleForm) {
        self.confirming_delete = false;
        self.problem = None;
        self.form = Some(form);
    }

    /// Opens the form of the rule at `index`.
    pub fn begin_edit(&mut self, index: usize) {
        if self.list_is_locked() {
            return;
        }
        let Some(listed) = self.rows.get(index) else {
            return;
        };
        self.cursor = index;
        self.confirming_delete = false;
        self.problem = None;
        self.form = Some(RuleForm::edit(&listed.rule));
    }

    /// Drops what was typed and closes the dialog; whether there was a form.
    pub fn cancel_form(&mut self) -> bool {
        self.form.take().is_some()
    }

    /// The write the form asks for, once and only while it is valid.
    pub fn begin_save(&mut self) -> Option<RuleSave> {
        self.form.as_mut()?.save_request()
    }

    /// Whether the page is of no use any more: made for a form that is gone.
    pub fn is_spent(&self) -> bool {
        self.form_only && self.form.is_none()
    }

    /// The write is done: the form closes. The caller loads the list again.
    pub fn saved(&mut self, id: RuleId) {
        self.select_on_load = Some(id);
        self.form = None;
        self.busy = true;
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
            .filter(|id| load.rules.iter().any(|row| row.rule.id == *id))
            .or_else(|| self.selected().map(|row| row.rule.id));
        self.cursor = selected
            .and_then(|id| load.rules.iter().position(|row| row.rule.id == id))
            .unwrap_or_else(|| self.cursor.min(load.rules.len().saturating_sub(1)));
        self.rows = load.rules;
        self.busy = false;
        self.confirming_delete = false;
        self.problem = None;
    }

    pub fn failed(&mut self, detail: String) {
        self.busy = false;
        self.confirming_delete = false;
        self.problem = Some(detail);
    }

    /// Flips the switch of the rule at `index` and returns the change to make.
    pub fn begin_toggle(&mut self, index: usize) -> Option<RuleChange> {
        if self.list_is_locked() {
            return None;
        }
        let row = self.rows.get(index)?;
        let change = RuleChange::SetEnabled {
            id: row.rule.id,
            enabled: !row.rule.spec.enabled,
        };
        self.begin(change)
    }

    /// Moves the rule at `index` one place among the rules of its target;
    /// the order of the other targets does not change.
    pub fn begin_move(&mut self, index: usize, delta: isize) -> Option<RuleChange> {
        if self.list_is_locked() {
            return None;
        }
        let row = self.rows.get(index)?;
        let place = self
            .rows_of(&row.rule.target)
            .position(|(listed, _)| listed == index)?;
        let to_index = place.checked_add_signed(delta)?;
        if to_index >= self.rows_of(&row.rule.target).count() {
            return None;
        }
        let id = row.rule.id;
        self.begin(RuleChange::Move { id, to_index })
    }

    /// The place in the list of the rule the form changes; `None` for a rule
    /// that is not listed yet.
    pub fn edited_index(&self) -> Option<usize> {
        let RuleFormTarget::Edit(rule) = &self.form.as_ref()?.target else {
            return None;
        };
        self.rows.iter().position(|row| row.rule.id == rule.id)
    }

    /// The question before the rule at `index` is removed. The form stays
    /// under it, backing out of the question leads back to it.
    pub fn ask_delete(&mut self, index: usize) {
        if !self.list_is_locked() && index < self.rows.len() {
            self.cursor = index;
            self.confirming_delete = true;
        }
    }

    pub fn begin_delete(&mut self) -> Option<RuleChange> {
        if self.list_is_locked() || !self.confirming_delete {
            return None;
        }
        let id = self.selected()?.rule.id;
        self.form = None;
        self.begin(RuleChange::Delete { id })
    }

    /// Backs out of the delete question; whether there was one.
    pub fn cancel_delete(&mut self) -> bool {
        std::mem::take(&mut self.confirming_delete)
    }

    fn begin(&mut self, change: RuleChange) -> Option<RuleChange> {
        self.busy = true;
        self.confirming_delete = false;
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
        (ConditionKind::Kind, false) => Msg::RulesSentenceKind,
        (ConditionKind::Kind, true) => Msg::RulesSentenceKindNot,
        (ConditionKind::ReadFolder, false) => Msg::RulesSentenceReadFolder,
        (ConditionKind::ReadFolder, true) => Msg::RulesSentenceReadFolderNot,
    }
}

fn shown_value<'a>(
    condition: &'a Condition,
    lang: Language,
    kinds: &'a KindCatalog,
) -> std::borrow::Cow<'a, str> {
    match condition.kind {
        ConditionKind::Kind => lang.kind_label(kinds, &condition.value),
        ConditionKind::ReadFolder => std::borrow::Cow::Borrowed(
            std::path::Path::new(condition.value.trim())
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(condition.value.as_str()),
        ),
        _ => std::borrow::Cow::Borrowed(condition.value.as_str()),
    }
}

fn condition_sentence(condition: &Condition, lang: Language, kinds: &KindCatalog) -> String {
    fill_template(
        lang.text(condition_template(condition.kind, condition.negated)),
        &[("{value}", &shown_value(condition, lang, kinds))],
    )
}

/// "Word in the text: policy and Sender: Company A": the conditions as a list
/// shows them.
pub fn conditions_short(spec: &RuleSpec, lang: Language, kinds: &KindCatalog) -> String {
    let joiner = lang.text(match spec.conditions.combinator {
        Combinator::AllOf => Msg::RulesJoinerAll,
        Combinator::AnyOf => Msg::RulesJoinerAny,
    });
    let mut shown: Vec<String> = Vec::new();
    let mut named = None;
    for condition in &spec.conditions.list {
        let value = shown_value(condition, lang, kinds);
        let name = lang.text(condition_kind_name(condition.kind));
        shown.push(if named == Some((condition.kind, condition.negated)) {
            value.into_owned()
        } else if condition.negated {
            format!("{name} {} {value}", lang.text(Msg::ConditionEditorHasNot))
        } else {
            format!("{name}: {value}")
        });
        named = Some((condition.kind, condition.negated));
    }
    shown.join(&format!(" {joiner} "))
}

/// One condition in the sentence; a tag reads as the bare tag.
fn sentence_part(
    condition: &Condition,
    first: bool,
    lang: Language,
    kinds: &KindCatalog,
) -> (bool, String) {
    let bare_tag = condition.kind == ConditionKind::Tag && (!condition.negated || !first);
    if bare_tag {
        (condition.negated, condition.value.clone())
    } else {
        (false, condition_sentence(condition, lang, kinds))
    }
}

/// "Kind is Invoice and recipient is Company A": the rule without what it
/// does.
fn conditions_text(spec: &RuleSpec, lang: Language, kinds: &KindCatalog) -> String {
    let combinator = spec.conditions.combinator;
    let mut conditions = String::new();
    for (position, condition) in spec.conditions.list.iter().enumerate() {
        let (not_joiner, text) = sentence_part(condition, position == 0, lang, kinds);
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
    conditions
}

/// The rule as one sentence: the conditions, then after the arrow the kind,
/// the tags to set and where the document moves to.
pub fn rule_sentence(spec: &RuleSpec, lang: Language, kinds: &KindCatalog) -> String {
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
            ("{conditions}", &conditions_text(spec, lang, kinds)),
            ("{target}", &target.join(" · ")),
        ],
    )
}

pub fn origin_text(origin: RuleOrigin, lang: Language) -> &'static str {
    lang.text(match origin {
        RuleOrigin::Hand => Msg::RulesOriginHand,
        RuleOrigin::Suggestion => Msg::RulesOriginSuggestion,
        RuleOrigin::Filing => Msg::RulesOriginFiling,
        RuleOrigin::Chat => Msg::RulesOriginChat,
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

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failing assertion is the signal"
)]
#[cfg(any(test, feature = "test-support"))]
pub mod fixtures;

#[cfg(test)]
// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
mod tests;
