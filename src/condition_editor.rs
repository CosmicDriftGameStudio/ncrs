//! The state of a list of conditions being edited: the rule form and the tax
//! contexts of the Hermes settings share it. No widgets; the view is
//! `ui::condition_editor`.

use ncrs_index::{
    compile_bounded_regex, iban_is_valid, Combinator, Condition, ConditionKind, RuleConditions,
};

/// The value fields need fixed ids, so the list has a cap.
pub const MAX_CONDITION_ROWS: usize = 12;

/// Every kind a rule offers.
pub const ALL_CONDITION_KINDS: [ConditionKind; 9] = [
    ConditionKind::Sender,
    ConditionKind::SenderAddress,
    ConditionKind::Recipient,
    ConditionKind::Word,
    ConditionKind::Substring,
    ConditionKind::Regex,
    ConditionKind::Iban,
    ConditionKind::TaxId,
    ConditionKind::Folder,
];

#[derive(Debug, Clone, PartialEq)]
pub struct ConditionEditor {
    pub combinator: Combinator,
    /// Edited in place: kind, value and negation.
    pub rows: Vec<Condition>,
    /// What the kind dropdown offers.
    pub kinds: &'static [ConditionKind],
    /// Whether the "not" checkbox is offered.
    pub negation: bool,
    /// Set by the first save attempt: a blank value is no problem before.
    pub show_empty_problems: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConditionEdit {
    Kind(usize, ConditionKind),
    Value(usize, String),
    Negated(usize, bool),
    Remove(usize),
    Add,
    Combinator(Combinator),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConditionProblem {
    Empty,
    /// The reason the regular expression does not compile, on one line.
    InvalidRegex(String),
    /// The checksum of the IBAN does not match.
    InvalidIban,
}

impl ConditionEditor {
    pub fn new(
        conditions: RuleConditions,
        kinds: &'static [ConditionKind],
        negation: bool,
    ) -> Self {
        Self {
            combinator: conditions.combinator,
            rows: conditions.list,
            kinds,
            negation,
            show_empty_problems: false,
        }
    }

    /// Out-of-range positions are ignored, and so is an `Add` at the cap.
    pub fn apply(&mut self, edit: ConditionEdit) {
        match edit {
            ConditionEdit::Kind(position, kind) => {
                if let Some(row) = self.rows.get_mut(position) {
                    row.kind = kind;
                }
            }
            ConditionEdit::Value(position, value) => {
                if let Some(row) = self.rows.get_mut(position) {
                    row.value = value;
                }
            }
            ConditionEdit::Negated(position, negated) => {
                if let Some(row) = self.rows.get_mut(position) {
                    row.negated = negated;
                }
            }
            ConditionEdit::Remove(position) => {
                if position < self.rows.len() {
                    self.rows.remove(position);
                }
            }
            ConditionEdit::Add => {
                if self.rows.len() < MAX_CONDITION_ROWS {
                    self.rows.push(Condition::new(self.default_kind(), ""));
                }
            }
            ConditionEdit::Combinator(combinator) => self.combinator = combinator,
        }
    }

    fn default_kind(&self) -> ConditionKind {
        self.kinds
            .first()
            .copied()
            .unwrap_or(ConditionKind::Substring)
    }

    pub fn can_add(&self) -> bool {
        self.rows.len() < MAX_CONDITION_ROWS
    }

    /// The kinds the dropdown of the row at `position` offers: the editor's
    /// own plus the row's kind, so a stored condition keeps it.
    pub fn kind_choices(&self, position: usize) -> Vec<ConditionKind> {
        let mut choices = self.kinds.to_vec();
        if let Some(row) = self.rows.get(position) {
            if !choices.contains(&row.kind) {
                choices.push(row.kind);
            }
        }
        choices
    }

    /// One entry per row.
    pub fn problems(&self) -> Vec<Option<ConditionProblem>> {
        self.rows
            .iter()
            .map(|row| row_problem(row, self.show_empty_problems))
            .collect()
    }

    pub fn mark_save_attempt(&mut self) {
        self.show_empty_problems = true;
    }

    /// Every row has a value and a regex compiles; unless `allow_empty_list`
    /// there is at least one row.
    pub fn is_valid(&self, allow_empty_list: bool) -> bool {
        (allow_empty_list || !self.rows.is_empty())
            && self.rows.iter().all(|row| row_problem(row, true).is_none())
    }

    /// The conditions as stored: values trimmed.
    pub fn conditions(&self) -> RuleConditions {
        RuleConditions {
            combinator: self.combinator,
            list: self
                .rows
                .iter()
                .map(|row| Condition {
                    value: row.value.trim().to_owned(),
                    ..row.clone()
                })
                .collect(),
        }
    }
}

fn row_problem(row: &Condition, report_empty: bool) -> Option<ConditionProblem> {
    let value = row.value.trim();
    if value.is_empty() {
        return report_empty.then_some(ConditionProblem::Empty);
    }
    match row.kind {
        ConditionKind::Regex => compile_bounded_regex(value)
            .err()
            .map(|err| ConditionProblem::InvalidRegex(reason_of(&err.to_string()))),
        ConditionKind::Iban => (!iban_is_valid(value)).then_some(ConditionProblem::InvalidIban),
        _ => None,
    }
}

/// The multi-line message of the regex crate names the reason on its last
/// line: `error: unclosed group`.
fn reason_of(message: &str) -> String {
    let last = message
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .trim();
    last.strip_prefix("error:")
        .unwrap_or(last)
        .trim()
        .to_owned()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    const TWO_KINDS: &[ConditionKind] = &[ConditionKind::Word, ConditionKind::Sender];

    fn editor_of(rows: Vec<Condition>) -> ConditionEditor {
        ConditionEditor::new(RuleConditions::all_of(rows), TWO_KINDS, true)
    }

    #[test]
    fn edits_change_the_row_in_place_and_ignore_a_missing_position() {
        let mut editor = editor_of(vec![Condition::new(ConditionKind::Word, "rent")]);

        editor.apply(ConditionEdit::Kind(0, ConditionKind::Sender));
        editor.apply(ConditionEdit::Value(0, "Telekom".to_owned()));
        editor.apply(ConditionEdit::Negated(0, true));
        editor.apply(ConditionEdit::Combinator(Combinator::AnyOf));
        let before = editor.clone();
        editor.apply(ConditionEdit::Kind(5, ConditionKind::Iban));
        editor.apply(ConditionEdit::Value(5, "x".to_owned()));
        editor.apply(ConditionEdit::Negated(5, true));
        editor.apply(ConditionEdit::Remove(5));

        assert_eq!(editor, before);
        assert_eq!(
            editor.rows,
            vec![Condition::not(ConditionKind::Sender, "Telekom")]
        );
        assert_eq!(editor.combinator, Combinator::AnyOf);
    }

    #[test]
    fn add_appends_an_empty_row_of_the_first_kind_and_remove_drops_one() {
        let mut editor = editor_of(Vec::new());

        editor.apply(ConditionEdit::Add);
        editor.apply(ConditionEdit::Add);
        editor.apply(ConditionEdit::Remove(0));

        assert_eq!(editor.rows, vec![Condition::new(ConditionKind::Word, "")]);
    }

    #[test]
    fn the_list_stops_growing_at_the_cap() {
        let mut editor = editor_of(Vec::new());

        for _ in 0..MAX_CONDITION_ROWS + 3 {
            editor.apply(ConditionEdit::Add);
        }

        assert_eq!(editor.rows.len(), MAX_CONDITION_ROWS);
        assert!(!editor.can_add());
    }

    #[test]
    fn a_blank_value_is_a_problem_only_after_the_first_save_attempt() {
        let mut editor = editor_of(vec![
            Condition::new(ConditionKind::Word, "  "),
            Condition::new(ConditionKind::Word, "rent"),
        ]);
        assert_eq!(editor.problems(), vec![None, None]);

        editor.mark_save_attempt();

        assert_eq!(editor.problems(), vec![Some(ConditionProblem::Empty), None]);
    }

    #[test]
    fn a_broken_regex_is_a_problem_at_once_with_the_reason() {
        let editor = editor_of(vec![
            Condition::new(ConditionKind::Regex, "("),
            Condition::new(ConditionKind::Regex, r"No\. \d+"),
            Condition::new(ConditionKind::Word, "("),
        ]);

        let problems = editor.problems();

        let Some(ConditionProblem::InvalidRegex(reason)) = &problems[0] else {
            panic!("no regex problem: {problems:?}");
        };
        assert!(!reason.contains('\n') && !reason.is_empty(), "{reason}");
        assert_eq!(problems[1..], [None, None]);
    }

    #[test]
    fn a_wrong_iban_is_a_problem_and_blocks_validity() {
        let editor = editor_of(vec![
            Condition::new(ConditionKind::Iban, "DE89 3704 0044 0532 0130 01"),
            Condition::new(ConditionKind::Iban, "DE89 3704 0044 0532 0130 00"),
            Condition::new(ConditionKind::Word, "DE89"),
        ]);

        assert_eq!(
            editor.problems(),
            vec![Some(ConditionProblem::InvalidIban), None, None]
        );
        assert!(!editor.is_valid(false));
    }

    #[test]
    fn validity_needs_values_good_regexes_and_a_row_unless_the_list_may_be_empty() {
        let empty = editor_of(Vec::new());
        assert!(empty.is_valid(true));
        assert!(!empty.is_valid(false));

        let blank = editor_of(vec![Condition::new(ConditionKind::Word, " ")]);
        assert!(!blank.is_valid(true));

        let broken = editor_of(vec![Condition::new(ConditionKind::Regex, "[")]);
        assert!(!broken.is_valid(false));

        let fine = editor_of(vec![Condition::new(ConditionKind::Word, "rent")]);
        assert!(fine.is_valid(false));
    }

    #[test]
    fn the_stored_conditions_have_trimmed_values() {
        let mut editor = editor_of(vec![Condition::not(ConditionKind::Word, "  rent ")]);
        editor.apply(ConditionEdit::Combinator(Combinator::AnyOf));

        assert_eq!(
            editor.conditions(),
            RuleConditions::any_of(vec![Condition::not(ConditionKind::Word, "rent")])
        );
    }

    #[test]
    fn a_stored_kind_the_editor_does_not_offer_stays_in_its_dropdown() {
        let editor = editor_of(vec![
            Condition::new(ConditionKind::Word, "rent"),
            Condition::new(ConditionKind::Folder, "Inbox"),
        ]);

        assert_eq!(editor.kind_choices(0), TWO_KINDS);
        assert_eq!(
            editor.kind_choices(1),
            [
                ConditionKind::Word,
                ConditionKind::Sender,
                ConditionKind::Folder
            ]
        );
    }
}
