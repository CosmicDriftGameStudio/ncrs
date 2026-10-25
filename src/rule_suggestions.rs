//! State of the rule suggestions view: the suggested rules, the cursor in
//! them, the form while one is edited, and the documents the selected one
//! matches. No widgets and no tasks; `App` owns it and changes it from
//! `update`, the view is `ui::rule_suggestions`.

use std::path::PathBuf;

use ncrs_index::{RuleId, RuleSpec, TextMatch};
use ncrs_intake::pattern_is_valid;

use crate::suggestion_work::{condition_text, SuggestedRule};

/// The fields of the form while a suggestion is edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub condition: String,
    pub kind: String,
    pub pattern: String,
}

/// Why a suggestion was not confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    NoCondition,
    NoKind,
    InvalidPattern,
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

#[derive(Debug, Clone, PartialEq)]
pub struct RuleSuggestions {
    rows: Vec<SuggestedRule>,
    cursor: usize,
    /// The analysis or a decision is running; the view takes no new one.
    busy: bool,
    edit: Option<Edit>,
    problem: Option<Problem>,
    matches: Matches,
    /// The suggestion `matches` belongs to.
    matches_of: Option<RuleId>,
}

impl RuleSuggestions {
    /// Opens busy: the analysis starts with the view.
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            cursor: 0,
            busy: true,
            edit: None,
            problem: None,
            matches: Matches::Unknown,
            matches_of: None,
        }
    }

    pub fn rows(&self) -> &[SuggestedRule] {
        &self.rows
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn selected(&self) -> Option<&SuggestedRule> {
        self.rows.get(self.cursor)
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    pub fn edit(&self) -> Option<&Edit> {
        self.edit.as_ref()
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
        self.edit = None;
    }

    /// The list the work returned. The cursor stays on the suggestion it was
    /// on, if that is still there.
    pub fn loaded(&mut self, rows: Vec<SuggestedRule>) {
        let selected = self.selected().map(|row| row.id);
        self.cursor = selected
            .and_then(|id| rows.iter().position(|row| row.id == id))
            .unwrap_or_else(|| self.cursor.min(rows.len().saturating_sub(1)));
        self.rows = rows;
        self.busy = false;
        self.edit = None;
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

    pub fn select(&mut self, index: usize) {
        if self.busy || index >= self.rows.len() || index == self.cursor {
            return;
        }
        self.cursor = index;
        self.edit = None;
        self.problem = None;
        self.forget_matches_of_others();
    }

    /// The documents to look up for the selected suggestion, once: asking
    /// again before the answer is in gives `None`.
    pub fn matches_wanted(&mut self) -> Option<(RuleId, String)> {
        let row = self.rows.get(self.cursor)?;
        if self.matches_of == Some(row.id) {
            return None;
        }
        let condition = condition_text(&row.spec)?.to_owned();
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

    pub fn start_edit(&mut self) {
        if self.busy {
            return;
        }
        let Some(row) = self.rows.get(self.cursor) else {
            return;
        };
        self.edit = Some(Edit {
            condition: condition_text(&row.spec).unwrap_or_default().to_owned(),
            kind: row.spec.kind.clone(),
            pattern: row.spec.filing_pattern.clone(),
        });
        self.problem = None;
    }

    pub fn cancel_edit(&mut self) {
        self.edit = None;
        self.problem = None;
    }

    pub fn set_condition(&mut self, text: String) {
        if let Some(edit) = self.edit.as_mut() {
            edit.condition = text;
            self.problem = None;
        }
    }

    pub fn set_kind(&mut self, text: String) {
        if let Some(edit) = self.edit.as_mut() {
            edit.kind = text;
            self.problem = None;
        }
    }

    pub fn set_pattern(&mut self, text: String) {
        if let Some(edit) = self.edit.as_mut() {
            edit.pattern = text;
            self.problem = None;
        }
    }

    /// The suggestion to confirm and the values to confirm it with: the edited
    /// ones while the form is up, else the suggested ones. The condition stays
    /// a literal "contains", never a pattern. An invalid pattern or an empty
    /// field is reported in the view and nothing is returned. Marks the view
    /// busy until the work answers.
    pub fn begin_confirm(&mut self) -> Option<(RuleId, RuleSpec)> {
        if self.busy {
            return None;
        }
        let row = self.rows.get(self.cursor)?;
        let (condition, kind, pattern) = match &self.edit {
            Some(edit) => (
                edit.condition.trim().to_owned(),
                edit.kind.trim().to_owned(),
                edit.pattern.trim().to_owned(),
            ),
            None => (
                condition_text(&row.spec).unwrap_or_default().to_owned(),
                row.spec.kind.clone(),
                row.spec.filing_pattern.clone(),
            ),
        };
        let problem = if condition.is_empty() {
            Some(Problem::NoCondition)
        } else if kind.is_empty() {
            Some(Problem::NoKind)
        } else if !pattern_is_valid(&pattern) {
            Some(Problem::InvalidPattern)
        } else {
            None
        };
        if problem.is_some() {
            self.problem = problem;
            return None;
        }
        let spec = RuleSpec {
            match_text: Some(TextMatch::Contains(condition)),
            kind,
            filing_pattern: pattern,
            ..row.spec.clone()
        };
        let id = row.id;
        self.busy = true;
        self.problem = None;
        Some((id, spec))
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

    use ncrs_index::{RuleEvidence, TaxContext};

    use super::*;

    fn spec(condition: &str, kind: &str) -> RuleSpec {
        RuleSpec {
            match_text: Some(TextMatch::Contains(condition.to_owned())),
            match_sender: None,
            extract_number: None,
            extract_date: None,
            extract_amount: None,
            kind: kind.to_owned(),
            tax_context: TaxContext::None,
            tags: BTreeSet::new(),
            filing_pattern: format!("{kind}/{{date}}_{{name}}"),
            confirmed: false,
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

        assert_eq!(first, Some((one, "company a ltd".to_owned())));
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
            Some((two, "agency b tax office".to_owned()))
        );
    }

    #[test]
    fn confirming_as_suggested_hands_back_the_suggested_values() {
        let [one, _, _] = rule_ids();
        let mut view = loaded_view();

        let (id, spec) = view.begin_confirm().unwrap();

        assert_eq!(id, one);
        assert_eq!(
            spec.match_text,
            Some(TextMatch::Contains("company a ltd".to_owned()))
        );
        assert_eq!(spec.kind, "Insurance");
        assert!(view.is_busy());
        assert_eq!(view.begin_confirm(), None, "one decision at a time");
    }

    #[test]
    fn an_edit_is_confirmed_with_the_edited_values_and_stays_a_literal_condition() {
        let mut view = loaded_view();
        view.start_edit();
        view.set_condition("  Company A (Ltd.)  ".to_owned());
        view.set_kind("Policy".to_owned());
        view.set_pattern("Policies/{year}/{name}".to_owned());

        let (_, spec) = view.begin_confirm().unwrap();

        assert_eq!(
            spec.match_text,
            Some(TextMatch::Contains("Company A (Ltd.)".to_owned()))
        );
        assert_eq!(spec.kind, "Policy");
        assert_eq!(spec.filing_pattern, "Policies/{year}/{name}");
    }

    #[test]
    fn an_invalid_pattern_or_an_empty_field_is_reported_and_nothing_is_confirmed() {
        for (field, expected) in [
            ("pattern", Problem::InvalidPattern),
            ("kind", Problem::NoKind),
            ("condition", Problem::NoCondition),
        ] {
            let mut view = loaded_view();
            view.start_edit();
            match field {
                "pattern" => view.set_pattern("{year}/{nonsense}".to_owned()),
                "kind" => view.set_kind("   ".to_owned()),
                _ => view.set_condition(String::new()),
            }

            assert_eq!(view.begin_confirm(), None, "{field}");

            assert_eq!(view.problem(), Some(&expected));
            assert!(!view.is_busy(), "the form stays usable: {field}");
            assert!(view.edit().is_some());
        }
    }

    #[test]
    fn typing_ends_the_complaint_and_moving_ends_the_edit() {
        let mut view = loaded_view();
        view.start_edit();
        view.set_pattern("{nonsense}".to_owned());
        view.begin_confirm();
        assert!(view.problem().is_some());

        view.set_pattern("{year}".to_owned());
        assert!(view.problem().is_none());

        view.move_cursor(1);
        assert!(view.edit().is_none());
    }

    #[test]
    fn nothing_is_decided_while_the_view_is_busy() {
        let mut view = RuleSuggestions::new();

        assert_eq!(view.begin_confirm(), None);
        assert_eq!(view.begin_discard(), None);
        view.start_edit();
        assert!(view.edit().is_none());
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
