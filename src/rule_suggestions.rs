//! State of the rule suggestions view: the suggested rules, the cursor in
//! them, and the documents the selected one matches. No widgets and no tasks; `App` owns it and changes it from
//! `update`, the view is `ui::rule_suggestions`.

use std::path::PathBuf;

use iced::mouse::ScrollDelta;

use ncrs_index::{Condition, Rule, RuleId, RuleOrigin, RuleSpec};

use crate::list_window::ListWindow;
use crate::suggestion_work::SuggestedRule;

/// Why a suggestion was not decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// The work on the index failed; the technical detail stays English.
    Failed(String),
}

/// The documents the selected suggestion matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Matches {
    /// Not asked for yet, or the selection moved on.
    Unknown,
    Looking,
    Found(Vec<PathBuf>),
}

/// Most suggestions of the list shown at once; the list follows the cursor.
pub const VISIBLE_ROWS: usize = 12;

#[derive(Debug, Clone, PartialEq)]
pub struct RuleSuggestions {
    rows: Vec<SuggestedRule>,
    cursor: usize,
    window: ListWindow,
    /// The analysis or a decision is running; the view takes no new one.
    busy: bool,
    problem: Option<Problem>,
    matches: Matches,
    /// The suggestion `matches` belongs to.
    matches_of: Option<RuleId>,
    /// The suggestion the first load puts the cursor on.
    select_on_load: Option<RuleId>,
}

impl RuleSuggestions {
    /// Opens busy: the analysis starts with the view.
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            cursor: 0,
            window: ListWindow::default(),
            busy: true,
            problem: None,
            matches: Matches::Unknown,
            matches_of: None,
            select_on_load: None,
        }
    }

    /// The next load puts the cursor on this suggestion, if it is listed.
    pub fn select_on_load(&mut self, id: RuleId) {
        self.select_on_load = Some(id);
    }

    pub fn rows(&self) -> &[SuggestedRule] {
        &self.rows
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The first suggestion of the list that is shown.
    pub fn first_visible(&self) -> usize {
        self.window.offset(self.rows.len(), VISIBLE_ROWS)
    }

    pub fn selected(&self) -> Option<&SuggestedRule> {
        self.rows.get(self.cursor)
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    pub fn problem(&self) -> Option<&Problem> {
        self.problem.as_ref()
    }

    pub fn matches(&self) -> &Matches {
        &self.matches
    }

    /// A new analysis starts; the list stays until it is replaced.
    pub fn begin_analysis(&mut self) {
        self.busy = true;
        self.problem = None;
    }

    /// The list the work returned. The cursor stays on the suggestion it was
    /// on, if that is still there.
    pub fn loaded(&mut self, rows: Vec<SuggestedRule>) {
        let selected = self
            .select_on_load
            .take()
            .or_else(|| self.selected().map(|row| row.id));
        self.cursor = selected
            .and_then(|id| rows.iter().position(|row| row.id == id))
            .unwrap_or_else(|| self.cursor.min(rows.len().saturating_sub(1)));
        self.rows = rows;
        self.window
            .ensure_visible(self.cursor, self.rows.len(), VISIBLE_ROWS);
        self.busy = false;
        self.problem = None;
        self.forget_matches_of_others();
    }

    pub fn failed(&mut self, detail: String) {
        self.busy = false;
        self.problem = Some(Problem::Failed(detail));
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
        self.problem = None;
        self.forget_matches_of_others();
    }

    /// The documents to look up for the selected suggestion, once: asking
    /// again before the answer is in gives `None`.
    pub fn matches_wanted(&mut self) -> Option<(RuleId, Condition)> {
        let row = self.rows.get(self.cursor)?;
        if self.matches_of == Some(row.id) {
            return None;
        }
        let condition = row.spec.conditions.single_condition()?.clone();
        let id = row.id;
        self.matches_of = Some(id);
        self.matches = Matches::Looking;
        Some((id, condition))
    }

    /// The answer to a lookup; one for a suggestion the cursor left is dropped.
    pub fn matches_found(&mut self, id: RuleId, paths: Vec<PathBuf>) {
        if self.matches_of == Some(id) {
            self.matches = Matches::Found(paths);
        }
    }

    /// The selected suggestion as the rule the form starts from. The place
    /// and origin are not written when the suggestion is confirmed.
    pub fn rule_to_edit(&self) -> Option<Rule> {
        if self.busy {
            return None;
        }
        let row = self.rows.get(self.cursor)?;
        Some(Rule {
            id: row.id,
            position: 0,
            origin: RuleOrigin::Suggestion,
            spec: row.spec.clone(),
        })
    }

    /// The suggestion to confirm as it was suggested. Marks the view busy
    /// until the work answers.
    pub fn begin_confirm(&mut self) -> Option<(RuleId, RuleSpec)> {
        if self.busy {
            return None;
        }
        let row = self.rows.get(self.cursor)?;
        let confirmed = (row.id, row.spec.clone());
        self.busy = true;
        self.problem = None;
        Some(confirmed)
    }

    /// The suggestion to discard; marks the view busy until the work answers.
    pub fn begin_discard(&mut self) -> Option<RuleId> {
        if self.busy {
            return None;
        }
        let id = self.rows.get(self.cursor)?.id;
        self.busy = true;
        self.problem = None;
        Some(id)
    }

    fn forget_matches_of_others(&mut self) {
        let selected = self.rows.get(self.cursor).map(|row| row.id);
        if self.matches_of != selected {
            self.matches_of = None;
            self.matches = Matches::Unknown;
        }
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
    use std::collections::BTreeSet;

    use ncrs_index::{ConditionKind, RuleConditions, RuleEvidence};

    use super::*;

    fn spec(condition: &str, kind: &str) -> RuleSpec {
        RuleSpec {
            name: None,
            conditions: RuleConditions::all_of(vec![Condition::new(
                ConditionKind::Substring,
                condition,
            )]),
            extract_number: None,
            extract_date: None,
            extract_amount: None,
            kind: Some(kind.to_owned()),
            tags: BTreeSet::new(),
            filing_pattern: format!("{kind}/{{date}}_{{name}}"),
            confirmed: false,
            enabled: true,
        }
    }

    pub fn suggestion(id: RuleId, condition: &str, kind: &str) -> SuggestedRule {
        SuggestedRule {
            id,
            spec: spec(condition, kind),
            evidence: RuleEvidence {
                matching_in_folder: 5,
                folder_documents: 5,
                matching_elsewhere: 0,
                subtree: Some(kind.to_owned()),
            },
        }
    }

    /// Ids come from a real index: only the index hands them out.
    pub fn rule_ids() -> [RuleId; 3] {
        let dir = tempfile::TempDir::new().unwrap();
        let mut index = ncrs_index::Index::create_plain(&dir.path().join("index.db")).unwrap();
        let spec = spec("x", "Letters");
        [(); 3].map(|()| index.create_rule(&spec).unwrap())
    }

    pub fn loaded_view() -> RuleSuggestions {
        let [one, two, _] = rule_ids();
        let mut view = RuleSuggestions::new();
        view.loaded(vec![
            suggestion(one, "company a ltd", "Insurance"),
            suggestion(two, "agency b tax office", "Taxes"),
        ]);
        view
    }

    #[test]
    fn the_view_opens_busy_and_empty() {
        let view = RuleSuggestions::new();

        assert!(view.is_busy());
        assert!(view.rows().is_empty());
        assert!(view.selected().is_none());
    }

    #[test]
    fn a_loaded_list_ends_the_busy_state_and_keeps_the_selected_suggestion() {
        let [_, two, three] = rule_ids();
        let mut view = loaded_view();
        view.select(1);

        view.begin_analysis();
        view.loaded(vec![
            suggestion(three, "other", "Letters"),
            suggestion(two, "agency b tax office", "Taxes"),
        ]);

        assert!(!view.is_busy());
        assert_eq!(view.selected().unwrap().id, two);
    }

    #[test]
    fn the_cursor_stays_inside_a_shorter_list() {
        let [_, _, three] = rule_ids();
        let mut view = loaded_view();
        view.select(1);

        view.loaded(vec![suggestion(three, "company c", "Letters")]);

        assert_eq!(view.cursor(), 0);
    }

    #[test]
    fn the_documents_of_a_suggestion_are_asked_for_once() {
        let [one, _, _] = rule_ids();
        let mut view = loaded_view();

        let first = view.matches_wanted();
        let again = view.matches_wanted();

        assert_eq!(
            first,
            Some((
                one,
                Condition::new(ConditionKind::Substring, "company a ltd")
            ))
        );
        assert_eq!(again, None);
        assert_eq!(view.matches(), &Matches::Looking);
        view.matches_found(one, vec![PathBuf::from("Insurance/a.pdf")]);
        assert_eq!(
            view.matches(),
            &Matches::Found(vec![PathBuf::from("Insurance/a.pdf")])
        );
    }

    #[test]
    fn moving_on_asks_for_the_next_suggestions_documents_and_drops_a_late_answer() {
        let [one, two, _] = rule_ids();
        let mut view = loaded_view();
        view.matches_wanted();

        view.move_cursor(1);
        view.matches_found(one, vec![PathBuf::from("late.pdf")]);

        assert_eq!(view.matches(), &Matches::Unknown);
        assert_eq!(
            view.matches_wanted(),
            Some((
                two,
                Condition::new(ConditionKind::Substring, "agency b tax office")
            ))
        );
    }

    #[test]
    fn confirming_as_suggested_hands_back_the_suggested_values() {
        let [one, _, _] = rule_ids();
        let mut view = loaded_view();

        let (id, spec) = view.begin_confirm().unwrap();

        assert_eq!(id, one);
        assert_eq!(spec.conditions.single_text(), Some("company a ltd"));
        assert_eq!(spec.kind.as_deref(), Some("Insurance"));
        assert!(view.is_busy());
        assert_eq!(view.begin_confirm(), None, "one decision at a time");
    }

    #[test]
    fn the_selected_suggestion_is_handed_to_the_form_as_a_rule() {
        let [_, two, _] = rule_ids();
        let mut view = loaded_view();
        view.select(1);

        let rule = view.rule_to_edit().unwrap();

        assert_eq!(rule.id, two);
        assert_eq!(rule.origin, RuleOrigin::Suggestion);
        assert_eq!(rule.spec, view.selected().unwrap().spec);
        assert!(!rule.spec.confirmed);
    }

    #[test]
    fn nothing_is_decided_while_the_view_is_busy() {
        let mut view = RuleSuggestions::new();

        assert_eq!(view.begin_confirm(), None);
        assert_eq!(view.begin_discard(), None);
        assert_eq!(view.rule_to_edit(), None);
    }

    #[test]
    fn a_failure_is_shown_and_ends_the_busy_state() {
        let mut view = loaded_view();
        view.begin_discard().unwrap();

        view.failed("index not open".to_owned());

        assert!(!view.is_busy());
        assert_eq!(
            view.problem(),
            Some(&Problem::Failed("index not open".to_owned()))
        );
    }
}
