//! State of the rules page: the confirmed rules in their order, the cursor in
//! them, the delete question, and the sentences the rules are shown as. No
//! widgets and no tasks; `App` owns it and changes it from `update`, the view
//! is `ui::rules`.

use iced::mouse::ScrollDelta;

use ncrs_index::{
    Combinator, Condition, ConditionKind, RuleOrigin, RuleSpec, TaxContext, TaxContextEntry,
};

use crate::app::fill_template;
use crate::i18n::{Language, Msg};
use crate::list_window::ListWindow;
use crate::rules_work::{ListedRule, RuleChange, RulesLoad};

/// Most rules of the list shown at once; the list follows the cursor.
pub const VISIBLE_ROWS: usize = 6;

#[derive(Debug, Clone, PartialEq)]
pub struct RulesPage {
    rows: Vec<ListedRule>,
    tax_contexts: Vec<TaxContextEntry>,
    cursor: usize,
    window: ListWindow,
    /// The list is loading or a change is running; the page takes no new one.
    busy: bool,
    confirming_delete: bool,
    /// The work on the index failed; the technical detail stays English.
    problem: Option<String>,
}

impl RulesPage {
    /// Opens busy: the list loads with the view.
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            tax_contexts: Vec::new(),
            cursor: 0,
            window: ListWindow::default(),
            busy: true,
            confirming_delete: false,
            problem: None,
        }
    }

    pub fn rows(&self) -> &[ListedRule] {
        &self.rows
    }

    pub fn tax_contexts(&self) -> &[TaxContextEntry] {
        &self.tax_contexts
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

    /// The list the work returned. The cursor stays on the rule it was on, if
    /// that is still there.
    pub fn loaded(&mut self, load: RulesLoad) {
        let selected = self.selected().map(|row| row.rule.id);
        self.cursor = selected
            .and_then(|id| load.rules.iter().position(|row| row.rule.id == id))
            .unwrap_or_else(|| self.cursor.min(load.rules.len().saturating_sub(1)));
        self.rows = load.rules;
        self.tax_contexts = load.tax_contexts;
        self.window
            .ensure_visible(self.cursor, self.rows.len(), VISIBLE_ROWS);
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

    pub fn select(&mut self, index: usize) {
        if self.busy || index >= self.rows.len() || index == self.cursor {
            return;
        }
        self.cursor = index;
        self.window
            .ensure_visible(index, self.rows.len(), VISIBLE_ROWS);
        self.confirming_delete = false;
        self.problem = None;
    }

    /// Flips the switch of the rule at `index` and returns the change to make.
    pub fn begin_toggle(&mut self, index: usize) -> Option<RuleChange> {
        if self.busy {
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
        if self.busy {
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
        if !self.busy && self.selected().is_some() {
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
    }
}

fn condition_sentence(condition: &Condition, lang: Language) -> String {
    fill_template(
        lang.text(condition_template(condition.kind, condition.negated)),
        &[("{value}", &condition.value)],
    )
}

fn tax_context_text(
    tax_context: Option<&TaxContext>,
    tax_contexts: &[TaxContextEntry],
    lang: Language,
) -> String {
    match tax_context {
        None => lang.text(Msg::RulesTaxOpen).to_owned(),
        Some(TaxContext::None) => lang.text(Msg::RulesTaxNone).to_owned(),
        Some(TaxContext::Named(short_name)) => tax_contexts
            .iter()
            .find(|entry| entry.short_name.eq_ignore_ascii_case(short_name))
            .map_or_else(|| short_name.clone(), |entry| entry.name.clone()),
    }
}

/// The rule as one sentence: the conditions, then after the arrow the kind,
/// the tax context, the tags and the filing pattern.
pub fn rule_sentence(spec: &RuleSpec, tax_contexts: &[TaxContextEntry], lang: Language) -> String {
    let joiner = format!(
        " {} ",
        lang.text(match spec.conditions.combinator {
            Combinator::AllOf => Msg::RulesJoinerAll,
            Combinator::AnyOf => Msg::RulesJoinerAny,
        })
    );
    let conditions = spec
        .conditions
        .list
        .iter()
        .map(|condition| condition_sentence(condition, lang))
        .collect::<Vec<_>>()
        .join(&joiner);
    let mut target = vec![
        lang.kind_label(&spec.kind).into_owned(),
        tax_context_text(spec.tax_context.as_ref(), tax_contexts, lang),
    ];
    if !spec.tags.is_empty() {
        target.push(
            spec.tags
                .iter()
                .map(|tag| format!("#{tag}"))
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    target.push(spec.filing_pattern.clone());
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
            conditions,
            extract_number: None,
            extract_date: None,
            extract_amount: None,
            kind: "Invoice".to_owned(),
            tax_context: Some(TaxContext::named("company")),
            tags: BTreeSet::from(["tax".to_owned(), "office".to_owned()]),
            filing_pattern: "{year}/{sender}".to_owned(),
            confirmed: true,
            enabled: true,
        }
    }

    fn entries() -> Vec<TaxContextEntry> {
        crate::review_list::tests::stored_tax_contexts()
    }

    fn listed(id: i64, global_index: usize) -> ListedRule {
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
            tax_contexts: entries(),
        });
        page
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
            tax_contexts: Vec::new(),
        });

        assert_eq!(page.selected().unwrap().rule.id, rule_id(3));
    }

    #[test]
    fn a_reload_after_a_delete_keeps_the_cursor_inside_the_list() {
        let mut page = page();
        page.select(2);

        page.loaded(RulesLoad {
            rules: vec![listed(1, 0), listed(2, 2)],
            tax_contexts: Vec::new(),
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
    fn a_failure_is_shown_and_frees_the_page() {
        let mut page = page();
        page.begin_toggle(0);

        page.failed("index not open".to_owned());

        assert!(!page.is_busy());
        assert_eq!(page.problem(), Some("index not open"));
    }

    #[test]
    fn a_sentence_joins_all_of_with_and_and_ends_in_the_target() {
        let spec = spec(RuleConditions::all_of(vec![
            Condition::new(ConditionKind::Word, "rent"),
            Condition::new(ConditionKind::Sender, "Company A"),
        ]));

        assert_eq!(
            rule_sentence(&spec, &entries(), Language::English),
            "If the text contains the word “rent” and the sender is “Company A” → \
             invoice · Firma · #office #tax · {year}/{sender}"
        );
    }

    #[test]
    fn a_sentence_joins_any_of_with_or_and_words_a_negation() {
        let spec = RuleSpec {
            tax_context: Some(TaxContext::None),
            tags: BTreeSet::new(),
            ..spec(RuleConditions::any_of(vec![
                Condition::not(ConditionKind::Sender, "Agency B"),
                Condition::new(ConditionKind::Regex, r"No\. \d+"),
            ]))
        };

        assert_eq!(
            rule_sentence(&spec, &entries(), Language::English),
            "If the sender is not “Agency B” or the text matches /No\\. \\d+/ → \
             invoice · no tax context · {year}/{sender}"
        );
    }

    #[test]
    fn a_german_sentence_uses_german_quotes_and_joiners() {
        let spec = RuleSpec {
            tax_context: None,
            tags: BTreeSet::new(),
            ..spec(RuleConditions::any_of(vec![
                Condition::not(ConditionKind::Word, "Mahnung"),
                Condition::new(ConditionKind::Folder, "Post/2024"),
            ]))
        };

        assert_eq!(
            rule_sentence(&spec, &entries(), Language::German),
            "Wenn der Text das Wort „Mahnung“ nicht enthält oder das Dokument in \
             „Post/2024“ liegt → Rechnung · Steuerkontext nach seinen Bedingungen · {year}/{sender}"
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
    fn a_named_context_that_is_not_stored_shows_its_short_name() {
        assert_eq!(
            tax_context_text(
                Some(&TaxContext::named("charity")),
                &entries(),
                Language::English
            ),
            "charity"
        );
    }

    #[test]
    fn hits_are_counted_in_singular_and_plural() {
        assert_eq!(hits_text(1, Language::English), "1 document");
        assert_eq!(hits_text(0, Language::English), "0 documents");
        assert_eq!(hits_text(12, Language::German), "12 Dokumente");
    }
}
