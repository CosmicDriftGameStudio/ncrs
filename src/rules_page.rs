//! State of the rules page: the confirmed rules in their order, the cursor in
//! them, the delete question, and the sentences the rules are shown as. No
//! widgets and no tasks; `App` owns it and changes it from `update`, the view
//! is `ui::rules`.

use iced::mouse::ScrollDelta;

use std::sync::Arc;

use ncrs_index::{Combinator, Condition, ConditionKind, KindCatalog, RuleId, RuleOrigin, RuleSpec};

use crate::app::fill_template;
use crate::filing_pieces::FilingTarget;
use crate::i18n::{Language, Msg};
use crate::list_window::ListWindow;
use crate::messages::HermesPage;
use crate::rule_form::{RuleForm, RuleFormTarget, RuleSave};
use crate::rules_work::{ListedRule, RuleChange, RulesLoad};

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

    /// The list takes no change while a change runs.
    fn list_is_locked(&self) -> bool {
        self.busy
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
        if self.list_is_locked() {
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
    pub fn saved(&mut self) -> Option<HermesPage> {
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
            &[("{count}", &crate::ui::format::grouped(hits))],
        )
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
pub mod tests {
    use std::collections::BTreeSet;

    use ncrs_index::{Index, Rule, RuleConditions, RuleId};

    use super::*;

    /// The `n`th id (from 1) an index hands out.
    pub fn rule_id(n: usize) -> RuleId {
        let dir = tempfile::TempDir::new().unwrap();
        let mut index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        let spec = spec(RuleConditions::all_of(vec![Condition::new(
            ConditionKind::Word,
            "rent",
        )]));
        let mut last = None;
        for _ in 0..n {
            last = Some(index.create_rule(&spec).unwrap());
        }
        last.unwrap()
    }

    fn spec(conditions: RuleConditions) -> RuleSpec {
        RuleSpec {
            name: None,
            conditions,
            extract_number: None,
            extract_date: None,
            extract_amount: None,
            kind: Some("Invoice".to_owned()),
            tags: BTreeSet::from(["tax".to_owned(), "office".to_owned()]),
            filing_pattern: "{year}/{sender}".to_owned(),
            confirmed: true,
            enabled: true,
        }
    }

    pub fn listed(id: i64, global_index: usize) -> ListedRule {
        ListedRule {
            rule: Rule {
                id: rule_id(usize::try_from(id).unwrap()),
                position: i64::try_from(global_index).unwrap() + 1,
                origin: RuleOrigin::Hand,
                spec: spec(RuleConditions::all_of(vec![Condition::new(
                    ConditionKind::Word,
                    "rent",
                )])),
            },
            global_index,
            hits: 0,
        }
    }

    /// Three listed rules with a suggestion between the first two (global
    /// places 0, 2 and 3).
    fn page() -> RulesPage {
        let mut page = RulesPage::new();
        page.loaded(RulesLoad {
            rules: vec![listed(1, 0), listed(2, 2), listed(3, 3)],
        });
        page
    }

    #[test]
    fn the_rule_to_select_on_load_gets_the_cursor_once() {
        let mut page = RulesPage::new();
        page.select_on_load(rule_id(3));

        page.loaded(RulesLoad {
            rules: vec![listed(1, 0), listed(2, 2), listed(3, 3)],
        });
        assert_eq!(page.selected().map(|row| row.rule.id), Some(rule_id(3)));

        page.select(0);
        page.loaded(RulesLoad {
            rules: vec![listed(1, 0), listed(2, 2), listed(3, 3)],
        });
        assert_eq!(page.selected().map(|row| row.rule.id), Some(rule_id(1)));
    }

    #[test]
    fn a_rule_to_select_that_is_not_listed_keeps_the_cursor() {
        let mut page = RulesPage::new();
        page.select_on_load(rule_id(9));

        page.loaded(RulesLoad {
            rules: vec![listed(1, 0), listed(2, 2)],
        });

        assert_eq!(page.selected().map(|row| row.rule.id), Some(rule_id(1)));
    }

    #[test]
    fn a_form_opened_from_elsewhere_survives_the_load_and_remembers_where_to_go_back() {
        let mut page = RulesPage::new();
        page.open_form(RuleForm::new(), Some(HermesPage::Review));

        page.loaded(RulesLoad::default());

        assert!(page.form().is_some());
        assert_eq!(page.saved(), Some(HermesPage::Review));
        assert_eq!(page.saved(), None);
    }

    #[test]
    fn cancelling_a_form_opened_from_elsewhere_names_the_page_to_go_back_to() {
        let mut page = page();
        page.open_form(RuleForm::new(), Some(HermesPage::RuleSuggestions));

        assert!(page.cancel_form());

        assert_eq!(page.take_return_to(), Some(HermesPage::RuleSuggestions));
    }

    #[test]
    fn a_form_of_the_page_itself_goes_back_nowhere() {
        let mut page = page();
        page.open_form(RuleForm::new(), Some(HermesPage::Review));
        assert!(page.cancel_form());
        page.take_return_to();

        page.begin_new();

        assert!(page.cancel_form());
        assert_eq!(page.take_return_to(), None);
    }

    #[test]
    fn a_new_page_is_busy_until_the_list_is_loaded() {
        let mut page = RulesPage::new();
        assert!(page.is_busy());

        page.loaded(RulesLoad::default());

        assert!(!page.is_busy());
        assert!(page.selected().is_none());
    }

    #[test]
    fn the_cursor_moves_inside_the_list() {
        let mut page = page();

        page.move_cursor(-1);
        assert_eq!(page.cursor(), 0);
        page.move_cursor(5);
        assert_eq!(page.cursor(), 2);
        page.select(1);
        assert_eq!(page.cursor(), 1);
    }

    #[test]
    fn a_reload_keeps_the_cursor_on_the_same_rule() {
        let mut page = page();
        page.select(2);

        page.loaded(RulesLoad {
            rules: vec![listed(3, 0), listed(1, 1), listed(2, 2)],
        });

        assert_eq!(page.selected().unwrap().rule.id, rule_id(3));
    }

    #[test]
    fn a_reload_after_a_delete_keeps_the_cursor_inside_the_list() {
        let mut page = page();
        page.select(2);

        page.loaded(RulesLoad {
            rules: vec![listed(1, 0), listed(2, 2)],
        });

        assert_eq!(page.cursor(), 1);
    }

    #[test]
    fn a_toggle_asks_for_the_opposite_of_the_current_switch() {
        let mut page = page();

        let change = page.begin_toggle(1);

        assert_eq!(
            change,
            Some(RuleChange::SetEnabled {
                id: rule_id(2),
                enabled: false
            })
        );
        assert_eq!(page.cursor(), 1);
        assert!(page.is_busy());
        assert_eq!(page.begin_toggle(0), None);
    }

    #[test]
    fn moving_down_targets_the_place_of_the_next_listed_rule_among_all() {
        let mut page = page();

        let change = page.begin_move(1);

        assert_eq!(
            change,
            Some(RuleChange::Move {
                id: rule_id(1),
                to_index: 2
            })
        );
    }

    #[test]
    fn moving_up_targets_the_place_of_the_previous_listed_rule_among_all() {
        let mut page = page();
        page.select(1);

        let change = page.begin_move(-1);

        assert_eq!(
            change,
            Some(RuleChange::Move {
                id: rule_id(2),
                to_index: 0
            })
        );
    }

    #[test]
    fn the_first_rule_cannot_move_up_and_the_last_cannot_move_down() {
        let mut page = page();
        assert_eq!(page.begin_move(-1), None);
        assert!(!page.is_busy());
        page.select(2);

        assert_eq!(page.begin_move(1), None);
        assert!(!page.is_busy());
    }

    #[test]
    fn deleting_needs_the_question_first() {
        let mut page = page();
        assert_eq!(page.begin_delete(), None);

        page.ask_delete();
        assert!(page.is_confirming_delete());
        let change = page.begin_delete();

        assert_eq!(change, Some(RuleChange::Delete { id: rule_id(1) }));
    }

    #[test]
    fn cancelling_the_question_deletes_nothing() {
        let mut page = page();
        page.ask_delete();

        assert!(page.cancel_delete());

        assert!(!page.is_confirming_delete());
        assert_eq!(page.begin_delete(), None);
        assert!(!page.cancel_delete());
    }

    #[test]
    fn moving_the_cursor_withdraws_the_question() {
        let mut page = page();
        page.ask_delete();

        page.move_cursor(1);

        assert!(!page.is_confirming_delete());
    }

    #[test]
    fn selecting_a_rule_drops_a_new_form_and_shows_the_form_of_the_rule() {
        let mut page = page();
        page.begin_new();
        assert!(page.has_unlisted_form());

        page.select(0);

        assert!(!page.has_unlisted_form());
        assert_eq!(page.cursor(), 0);
        assert!(!page.is_editing());
        assert!(matches!(
            page.form().unwrap().target,
            crate::rule_form::RuleFormTarget::Edit(_)
        ));
        page.begin_new();
        page.move_cursor(1);
        assert_eq!(page.cursor(), 1);
        assert!(!page.has_unlisted_form());
    }

    #[test]
    fn while_a_change_runs_the_list_takes_no_change() {
        let mut page = page();
        assert!(page.begin_toggle(0).is_some());

        page.move_cursor(1);
        page.select(2);
        page.ask_delete();
        page.begin_new();

        assert_eq!(page.cursor(), 0);
        assert!(!page.is_confirming_delete());
        assert!(!page.has_unlisted_form());
        assert_eq!(page.begin_toggle(0), None);
        assert_eq!(page.begin_move(1), None);
    }

    #[test]
    fn a_form_that_was_typed_into_is_changed_until_cancelled() {
        let mut page = page();
        assert!(!page.form().unwrap().is_changed());

        page.form_mut()
            .unwrap()
            .update(crate::messages::RuleFormMessage::Name(
                "Contract".to_owned(),
            ));
        assert!(page.form().unwrap().is_changed());

        assert!(page.cancel_form());
        assert!(!page.form().unwrap().is_changed());
    }

    #[test]
    fn saving_a_new_form_creates_the_rule_and_an_edit_form_changes_it() {
        use crate::condition_editor::ConditionEdit;
        use crate::filing_pieces::TargetEdit;
        use crate::messages::RuleFormMessage;
        use crate::rules_work::RulesAccess;
        use std::sync::{Arc, Mutex};

        let dir = tempfile::TempDir::new().unwrap();
        let index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        let access = RulesAccess::new(Arc::new(Mutex::new(Some(index))), Vec::new());
        let mut page = RulesPage::new();
        page.loaded(access.load().unwrap());

        page.begin_new();
        assert_eq!(page.begin_save(), None);
        page.form_mut()
            .unwrap()
            .update(RuleFormMessage::Condition(ConditionEdit::Value(
                0,
                "telekom".to_owned(),
            )));
        access.save(page.begin_save().unwrap()).unwrap();
        page.saved();
        page.loaded(access.load().unwrap());

        assert!(!page.has_unlisted_form());
        let sentence = rule_sentence(
            &page.selected().unwrap().rule.spec,
            Language::English,
            &KindCatalog::built_in(),
        );
        assert_eq!(sentence, "If telekom → Year / Sender");

        page.begin_edit();
        page.form_mut()
            .unwrap()
            .update(RuleFormMessage::Target(TargetEdit::Remove(1)));
        access.save(page.begin_save().unwrap()).unwrap();
        page.saved();
        page.loaded(access.load().unwrap());

        assert_eq!(page.rows().len(), 1);
        assert_eq!(page.selected().unwrap().rule.spec.filing_pattern, "{year}/");
    }

    #[test]
    fn a_failed_save_keeps_the_form_with_the_reason() {
        let mut page = page();
        page.begin_edit();
        page.begin_save().unwrap();

        page.save_failed("index not open".to_owned());

        assert_eq!(
            page.form().unwrap().failure.as_deref(),
            Some("index not open")
        );
        assert!(page.begin_save().is_some());
    }

    #[test]
    fn a_failure_is_shown_and_frees_the_page() {
        let mut page = page();
        page.begin_toggle(0);

        page.failed("index not open".to_owned());

        assert!(!page.is_busy());
        assert_eq!(page.problem(), Some("index not open"));
    }

    #[test]
    fn a_sentence_joins_all_of_with_and_and_ends_in_the_target() {
        let plain = spec(RuleConditions::all_of(vec![
            Condition::new(ConditionKind::Word, "rent"),
            Condition::new(ConditionKind::Sender, "Company A"),
        ]));
        let spec = RuleSpec {
            filing_pattern: "{year}/{sender}/".to_owned(),
            ..plain
        };

        assert_eq!(
            rule_sentence(&spec, Language::English, &KindCatalog::built_in()),
            "If the text contains the word “rent” and the sender is “Company A” → \
             Invoice · “office” “tax” · Year / Sender"
        );
    }

    #[test]
    fn a_sentence_joins_any_of_with_or_and_words_a_negation() {
        let spec = RuleSpec {
            tags: BTreeSet::new(),
            filing_pattern: "{year}/{sender}/".to_owned(),
            ..spec(RuleConditions::any_of(vec![
                Condition::not(ConditionKind::Sender, "Agency B"),
                Condition::new(ConditionKind::Regex, r"No\. \d+"),
            ]))
        };

        assert_eq!(
            rule_sentence(&spec, Language::English, &KindCatalog::built_in()),
            "If the sender is not “Agency B” or the text matches /No\\. \\d+/ → \
             Invoice · Year / Sender"
        );
    }

    #[test]
    fn a_german_sentence_uses_german_quotes_and_joiners() {
        let spec = RuleSpec {
            tags: BTreeSet::new(),
            filing_pattern: "{year}/{sender}/".to_owned(),
            ..spec(RuleConditions::any_of(vec![
                Condition::not(ConditionKind::Word, "Mahnung"),
                Condition::new(ConditionKind::Folder, "Post/2024"),
            ]))
        };

        assert_eq!(
            rule_sentence(&spec, Language::German, &KindCatalog::built_in()),
            "Wenn der Text das Wort „Mahnung“ nicht enthält oder das Dokument in \
             „Post/2024“ liegt → Rechnung · Jahr / Absender"
        );
    }

    #[test]
    fn every_kind_has_a_sentence_in_both_languages_with_and_without_negation() {
        for lang in [Language::English, Language::German] {
            for kind in ConditionKind::ALL {
                for negated in [false, true] {
                    let condition = Condition {
                        kind,
                        value: "VALUE".to_owned(),
                        negated,
                    };
                    let sentence = condition_sentence(&condition, lang);
                    assert!(sentence.contains("VALUE"), "{kind:?} {negated}: {sentence}");
                    assert!(!sentence.contains("{value}"), "{sentence}");
                }
            }
        }
    }

    #[test]
    fn hits_are_counted_in_singular_and_plural() {
        assert_eq!(hits_text(1, Language::English), "1 hit");
        assert_eq!(hits_text(0, Language::English), "0 hits");
        assert_eq!(hits_text(12, Language::German), "12 Treffer");
    }
}
