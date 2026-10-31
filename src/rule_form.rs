//! The form of a rule: a new one, or an existing one being changed. Nothing is
//! stored before Save. No widgets, the view is `ui::rules`.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use chrono::NaiveDate;
use ncrs_index::{
    Condition, ConditionKind, DocumentFields, Rule, RuleConditions, RuleId, RuleOrigin, RuleSpec,
};

use crate::condition_editor::{ConditionEditor, ConditionProblem, ALL_CONDITION_KINDS};
use crate::messages::RuleFormMessage;
use crate::review_work::{parse_tags, tags_text};
use crate::rules_work::{RuleTry, TryResult};

const NEW_RULE_PATTERN: &str = "{year}/{sender}";
const EXAMPLE_KIND: &str = "invoice";
const EXAMPLE_SENDER: &str = "Telekom";
const EXAMPLE_NUMBER: &str = "4711";

/// How long the form rests after an edit before the try runs.
pub const TRY_DEBOUNCE: Duration = Duration::from_millis(300);

/// Taken from one counter for all forms, so that the late answer of a form
/// that was closed never fits the generation of the next one.
static LAST_TRY_GENERATION: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq)]
pub enum TryStatus {
    Idle,
    Waiting,
    Running,
    Ready(TryResult),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TryState {
    pub generation: u64,
    pub status: TryStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RuleFormTarget {
    New,
    /// Made from a document the user filed by hand.
    FromDocument,
    Edit(Rule),
    /// A suggestion the user looks over before it becomes a rule.
    FromSuggestion(Rule),
}

/// What a form made from a document starts with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulePrefill {
    pub sender: String,
    pub kind: String,
    pub tags: BTreeSet<String>,
    pub pattern: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuleForm {
    pub target: RuleFormTarget,
    pub conditions: ConditionEditor,
    pub kind: String,
    /// Comma separated, like the review list.
    pub tags: String,
    pub pattern: String,
    pub saving: bool,
    /// The index refused the rule; the technical detail stays English.
    pub failure: Option<String>,
    /// The documents the rule as it stands in the form would match.
    pub live_try: TryState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternProblem {
    Blank,
    /// The name of a `{block}` the pattern does not know.
    UnknownBlock(String),
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleFormProblems {
    /// One entry per condition row.
    pub conditions: Vec<Option<ConditionProblem>>,
    /// A rule without a condition would match every document.
    pub no_condition: bool,
    pub kind_blank: bool,
    pub pattern: Option<PatternProblem>,
}

/// What to write to the index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleSave {
    Create {
        spec: RuleSpec,
        origin: RuleOrigin,
    },
    Update(RuleId, RuleSpec),
    /// The suggestion becomes a confirmed rule with these values.
    ConfirmSuggestion(RuleId, RuleSpec),
}

impl RuleForm {
    pub fn new() -> Self {
        Self {
            target: RuleFormTarget::New,
            conditions: ConditionEditor::new(
                RuleConditions::all_of(vec![Condition::new(ConditionKind::Sender, "")]),
                &ALL_CONDITION_KINDS,
                true,
            ),
            kind: String::new(),
            tags: String::new(),
            pattern: NEW_RULE_PATTERN.to_owned(),
            saving: false,
            failure: None,
            live_try: TryState {
                generation: 0,
                status: TryStatus::Idle,
            },
        }
    }

    pub fn edit(rule: &Rule) -> Self {
        Self::of_rule(rule, RuleFormTarget::Edit(rule.clone()))
    }

    pub fn from_suggestion(rule: &Rule) -> Self {
        Self::of_rule(rule, RuleFormTarget::FromSuggestion(rule.clone()))
    }

    pub fn from_document(prefill: RulePrefill) -> Self {
        let pattern = if prefill.pattern.trim().is_empty() {
            NEW_RULE_PATTERN.to_owned()
        } else {
            prefill.pattern
        };
        Self {
            target: RuleFormTarget::FromDocument,
            conditions: ConditionEditor::new(
                RuleConditions::all_of(vec![Condition::new(ConditionKind::Sender, prefill.sender)]),
                &ALL_CONDITION_KINDS,
                true,
            ),
            kind: prefill.kind,
            tags: tags_text(&prefill.tags),
            pattern,
            saving: false,
            failure: None,
            live_try: TryState {
                generation: 0,
                status: TryStatus::Idle,
            },
        }
    }

    fn of_rule(rule: &Rule, target: RuleFormTarget) -> Self {
        let spec = &rule.spec;
        Self {
            target,
            conditions: ConditionEditor::new(spec.conditions.clone(), &ALL_CONDITION_KINDS, true),
            kind: spec.kind.clone(),
            tags: tags_text(&spec.tags),
            pattern: spec.filing_pattern.clone(),
            saving: false,
            failure: None,
            live_try: TryState {
                generation: 0,
                status: TryStatus::Idle,
            },
        }
    }

    /// Applies the messages that change a field; the others are the page's.
    pub fn update(&mut self, message: RuleFormMessage) {
        match message {
            RuleFormMessage::Condition(edit) => self.conditions.apply(edit),
            RuleFormMessage::Kind(kind) => self.kind = kind,
            RuleFormMessage::Tags(tags) => self.tags = tags,
            RuleFormMessage::Pattern(pattern) => self.pattern = pattern,
            RuleFormMessage::InsertBlock(name) => {
                self.pattern.push('{');
                self.pattern.push_str(name);
                self.pattern.push('}');
            }
            RuleFormMessage::Save | RuleFormMessage::Cancel | RuleFormMessage::Saved { .. } => {
                return;
            }
        }
        self.failure = None;
    }

    /// An edit makes the answer to come stale: waits for the form to rest.
    /// The generation to hand to `try_request` once it did.
    pub fn restart_try(&mut self) -> u64 {
        let generation = LAST_TRY_GENERATION
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1);
        self.live_try = TryState {
            generation,
            status: TryStatus::Waiting,
        };
        generation
    }

    /// The rest is over: the try to run, unless the form was edited since or
    /// its conditions cannot be tried. Never marks a save attempt.
    pub fn try_request(&mut self, generation: u64) -> Option<RuleTry> {
        if generation != self.live_try.generation {
            return None;
        }
        if !self.conditions.is_valid(false) {
            self.live_try.status = TryStatus::Idle;
            return None;
        }
        self.live_try.status = TryStatus::Running;
        Some(RuleTry {
            replaces: self.replaced_rule(),
            spec: self.form_spec(),
        })
    }

    /// The answer of the try; one of an older generation is dropped.
    pub fn try_answered(&mut self, generation: u64, result: Result<TryResult, String>) {
        if generation != self.live_try.generation {
            return;
        }
        self.live_try.status = match result {
            Ok(found) => TryStatus::Ready(found),
            Err(detail) => TryStatus::Failed(detail),
        };
    }

    fn replaced_rule(&self) -> Option<RuleId> {
        match &self.target {
            RuleFormTarget::Edit(rule) | RuleFormTarget::FromSuggestion(rule) => Some(rule.id),
            RuleFormTarget::New | RuleFormTarget::FromDocument => None,
        }
    }

    /// What the form describes; an existing rule keeps what the form does not
    /// show.
    fn form_spec(&self) -> RuleSpec {
        let kept = match &self.target {
            RuleFormTarget::Edit(rule) | RuleFormTarget::FromSuggestion(rule) => Some(&rule.spec),
            RuleFormTarget::New | RuleFormTarget::FromDocument => None,
        };
        RuleSpec {
            conditions: self.conditions.conditions(),
            extract_number: kept.and_then(|spec| spec.extract_number.clone()),
            extract_date: kept.and_then(|spec| spec.extract_date.clone()),
            extract_amount: kept.and_then(|spec| spec.extract_amount.clone()),
            kind: self.kind.trim().to_owned(),
            tags: parse_tags(&self.tags),
            filing_pattern: self.pattern.trim().to_owned(),
            confirmed: kept.is_none_or(|spec| spec.confirmed),
            enabled: kept.is_none_or(|spec| spec.enabled),
        }
    }

    fn save_was_tried(&self) -> bool {
        self.conditions.show_empty_problems
    }

    pub fn problems(&self) -> RuleFormProblems {
        RuleFormProblems {
            conditions: self.conditions.problems(),
            no_condition: self.save_was_tried() && self.conditions.rows.is_empty(),
            kind_blank: self.save_was_tried() && self.kind.trim().is_empty(),
            pattern: self.pattern_problem(self.save_was_tried()),
        }
    }

    fn pattern_problem(&self, report_blank: bool) -> Option<PatternProblem> {
        let pattern = self.pattern.trim();
        if pattern.is_empty() {
            return report_blank.then_some(PatternProblem::Blank);
        }
        if let Some(name) = ncrs_intake::unknown_placeholder(pattern) {
            return Some(PatternProblem::UnknownBlock(name));
        }
        (!ncrs_intake::pattern_is_valid(pattern)).then_some(PatternProblem::Invalid)
    }

    pub fn is_valid(&self) -> bool {
        self.conditions.is_valid(false)
            && !self.kind.trim().is_empty()
            && self.pattern_problem(true).is_none()
    }

    /// The path the pattern gives for a sample document, in the kind of the
    /// form, folders joined with `/`.
    pub fn example_path(&self) -> Option<String> {
        let kind = self.kind.trim();
        let fields = DocumentFields {
            sender: Some(EXAMPLE_SENDER.to_owned()),
            kind: Some(if kind.is_empty() { EXAMPLE_KIND } else { kind }.to_owned()),
            number: Some(EXAMPLE_NUMBER.to_owned()),
            date: NaiveDate::from_ymd_opt(2024, 3, 15),
            ..DocumentFields::default()
        };
        let path = ncrs_intake::render_pattern(self.pattern.trim(), &fields)?;
        Some(
            path.components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/"),
        )
    }

    /// The write to make, once; the answer comes to the page's `saved`.
    pub fn save_request(&mut self) -> Option<RuleSave> {
        self.conditions.mark_save_attempt();
        if self.saving || !self.is_valid() {
            return None;
        }
        self.saving = true;
        self.failure = None;
        let spec = self.form_spec();
        Some(match &self.target {
            RuleFormTarget::New => RuleSave::Create {
                spec,
                origin: RuleOrigin::Hand,
            },
            RuleFormTarget::FromDocument => RuleSave::Create {
                spec,
                origin: RuleOrigin::Filing,
            },
            RuleFormTarget::Edit(rule) => RuleSave::Update(rule.id, spec),
            RuleFormTarget::FromSuggestion(rule) => RuleSave::ConfirmSuggestion(rule.id, spec),
        })
    }

    /// The write failed: the form stays open with the reason.
    pub fn save_failed(&mut self, detail: String) {
        self.saving = false;
        self.failure = Some(detail);
    }
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
    use crate::condition_editor::ConditionEdit;
    use crate::rules_page::tests::rule_id;

    fn stored_rule() -> Rule {
        Rule {
            id: rule_id(1),
            position: 1,
            origin: RuleOrigin::Suggestion,
            spec: RuleSpec {
                conditions: RuleConditions::any_of(vec![
                    Condition::new(ConditionKind::Word, "rent"),
                    Condition::not(ConditionKind::Sender, "Agency B"),
                ]),
                extract_number: None,
                extract_date: Some(r"\d{4}-\d{2}-\d{2}".to_owned()),
                extract_amount: None,
                kind: "Invoice".to_owned(),
                tags: BTreeSet::from(["tax".to_owned(), "office".to_owned()]),
                filing_pattern: "{year}/{sender}".to_owned(),
                confirmed: true,
                enabled: false,
            },
        }
    }

    fn filled_new() -> RuleForm {
        let mut form = RuleForm::new();
        form.update(RuleFormMessage::Condition(ConditionEdit::Value(
            0,
            "  Telekom ".to_owned(),
        )));
        form.update(RuleFormMessage::Kind(" Invoice ".to_owned()));
        form
    }

    #[test]
    fn a_new_form_starts_with_one_empty_sender_row_and_the_usual_pattern() {
        let form = RuleForm::new();

        assert_eq!(form.conditions.rows.len(), 1);
        assert_eq!(form.conditions.rows[0].kind, ConditionKind::Sender);
        assert_eq!(form.conditions.rows[0].value, "");
        assert_eq!(form.pattern, "{year}/{sender}");
        assert!(!form.is_valid());
    }

    #[test]
    fn an_edit_form_starts_from_the_rule() {
        let form = RuleForm::edit(&stored_rule());

        assert_eq!(form.kind, "Invoice");
        assert_eq!(form.tags, "office, tax");
        assert_eq!(form.conditions.rows.len(), 2);
        assert!(form.is_valid());
    }

    #[test]
    fn inserting_a_block_appends_it_without_a_slash() {
        let mut form = RuleForm::new();

        form.update(RuleFormMessage::InsertBlock("number"));

        assert_eq!(form.pattern, "{year}/{sender}{number}");
    }

    #[test]
    fn nothing_is_reported_blank_before_the_first_save_attempt() {
        let mut form = RuleForm::new();
        form.update(RuleFormMessage::Pattern(String::new()));
        assert_eq!(form.problems().pattern, None);
        assert!(!form.problems().kind_blank);

        assert_eq!(form.save_request(), None);

        let problems = form.problems();
        assert_eq!(problems.pattern, Some(PatternProblem::Blank));
        assert!(problems.kind_blank);
        assert_eq!(problems.conditions, vec![Some(ConditionProblem::Empty)]);
    }

    #[test]
    fn an_unknown_block_and_a_broken_pattern_are_reported_at_once() {
        let mut form = RuleForm::new();

        form.update(RuleFormMessage::Pattern("{year}/{foo}".to_owned()));
        assert_eq!(
            form.problems().pattern,
            Some(PatternProblem::UnknownBlock("foo".to_owned()))
        );
        form.update(RuleFormMessage::Pattern("{year}//{sender}".to_owned()));
        assert_eq!(form.problems().pattern, Some(PatternProblem::Invalid));
    }

    #[test]
    fn a_rule_without_a_condition_cannot_be_saved() {
        let mut form = filled_new();
        form.update(RuleFormMessage::Condition(ConditionEdit::Remove(0)));

        assert_eq!(form.save_request(), None);
        assert!(form.problems().no_condition);
    }

    #[test]
    fn the_example_uses_the_sample_document_and_the_form_kind() {
        let mut form = RuleForm::new();
        assert_eq!(form.example_path().as_deref(), Some("2024/Telekom"));

        form.update(RuleFormMessage::Pattern(
            "{kind}/{year}/{number}".to_owned(),
        ));
        assert_eq!(form.example_path().as_deref(), Some("invoice/2024/4711"));
        form.update(RuleFormMessage::Kind("Receipt".to_owned()));
        assert_eq!(form.example_path().as_deref(), Some("Receipt/2024/4711"));
        form.update(RuleFormMessage::Pattern("{foo}".to_owned()));
        assert_eq!(form.example_path(), None);
    }

    #[test]
    fn a_new_form_saves_a_confirmed_enabled_rule_once() {
        let mut form = filled_new();

        let first = form.save_request().unwrap();
        let second = form.save_request();

        assert_eq!(second, None);
        let RuleSave::Create { spec, origin } = first else {
            panic!("not a create");
        };
        assert_eq!(origin, RuleOrigin::Hand);
        assert_eq!(
            spec.conditions,
            RuleConditions::all_of(vec![Condition::new(ConditionKind::Sender, "Telekom")])
        );
        assert_eq!(spec.kind, "Invoice");
        assert_eq!(spec.filing_pattern, "{year}/{sender}");
        assert!(spec.confirmed && spec.enabled);
        assert!(spec.extract_date.is_none() && spec.extract_number.is_none());
    }

    #[test]
    fn an_edit_form_keeps_what_it_does_not_show() {
        let rule = stored_rule();
        let mut form = RuleForm::edit(&rule);
        form.update(RuleFormMessage::Pattern("{year}".to_owned()));
        form.update(RuleFormMessage::Tags("tax, , home ".to_owned()));

        let RuleSave::Update(id, spec) = form.save_request().unwrap() else {
            panic!("not an update");
        };

        assert_eq!(id, rule.id);
        assert_eq!(spec.filing_pattern, "{year}");
        assert_eq!(
            spec.tags,
            BTreeSet::from(["home".to_owned(), "tax".to_owned()])
        );
        assert_eq!(spec.extract_date, rule.spec.extract_date);
        assert_eq!(spec.conditions, rule.spec.conditions);
        assert!(!spec.enabled && spec.confirmed);
    }

    #[test]
    fn a_failed_save_keeps_the_form_and_allows_another_try() {
        let mut form = filled_new();
        form.save_request().unwrap();

        form.save_failed("invalid regex".to_owned());

        assert_eq!(form.failure.as_deref(), Some("invalid regex"));
        assert!(form.save_request().is_some());
        assert_eq!(form.failure, None);
    }

    fn prefill() -> RulePrefill {
        RulePrefill {
            sender: "Telekom".to_owned(),
            kind: "invoice".to_owned(),
            tags: BTreeSet::from(["tax".to_owned()]),
            pattern: "{sender}/{date}".to_owned(),
        }
    }

    #[test]
    fn a_form_from_a_document_starts_with_its_sender_and_fields() {
        let form = RuleForm::from_document(prefill());

        assert_eq!(form.target, RuleFormTarget::FromDocument);
        assert_eq!(
            form.conditions.conditions(),
            RuleConditions::all_of(vec![Condition::new(ConditionKind::Sender, "Telekom")])
        );
        assert_eq!(form.kind, "invoice");
        assert_eq!(form.tags, "tax");
        assert_eq!(form.pattern, "{sender}/{date}");
        assert!(form.is_valid());
    }

    #[test]
    fn a_form_from_a_document_without_a_pattern_takes_the_usual_one() {
        let form = RuleForm::from_document(RulePrefill {
            pattern: "  ".to_owned(),
            sender: String::new(),
            ..prefill()
        });

        assert_eq!(form.pattern, "{year}/{sender}");
        assert_eq!(form.conditions.rows.len(), 1);
        assert_eq!(form.conditions.rows[0].value, "");
        assert!(!form.is_valid());
    }

    #[test]
    fn a_form_from_a_document_saves_a_rule_with_the_origin_filing() {
        let mut form = RuleForm::from_document(prefill());

        let RuleSave::Create { spec, origin } = form.save_request().unwrap() else {
            panic!("not a create");
        };

        assert_eq!(origin, RuleOrigin::Filing);
        assert!(spec.confirmed && spec.enabled);
        assert_eq!(spec.tags, BTreeSet::from(["tax".to_owned()]));
    }

    #[test]
    fn a_form_from_a_suggestion_confirms_it_and_keeps_what_it_does_not_show() {
        let rule = stored_rule();
        let mut form = RuleForm::from_suggestion(&rule);
        assert_eq!(form.target, RuleFormTarget::FromSuggestion(rule.clone()));
        form.update(RuleFormMessage::Pattern("{year}".to_owned()));

        let RuleSave::ConfirmSuggestion(id, spec) = form.save_request().unwrap() else {
            panic!("not a confirmation");
        };

        assert_eq!(id, rule.id);
        assert_eq!(spec.filing_pattern, "{year}");
        assert_eq!(spec.extract_date, rule.spec.extract_date);
        assert_eq!(spec.conditions, rule.spec.conditions);
    }

    #[test]
    fn an_edit_waits_and_only_the_latest_generation_sends_a_request() {
        let mut form = filled_new();
        let first = form.restart_try();
        assert_eq!(form.live_try.status, TryStatus::Waiting);
        form.update(RuleFormMessage::Pattern("{year}".to_owned()));
        let second = form.restart_try();

        assert_ne!(first, second);
        assert_eq!(form.try_request(first), None);
        assert_eq!(form.live_try.status, TryStatus::Waiting);
        let request = form.try_request(second).unwrap();
        assert_eq!(form.live_try.status, TryStatus::Running);
        assert_eq!(request.replaces, None);
        assert_eq!(request.spec.filing_pattern, "{year}");
        assert!(request.spec.confirmed && request.spec.enabled);
        assert!(!form.conditions.show_empty_problems);
    }

    #[test]
    fn an_answer_of_an_older_generation_is_dropped() {
        let mut form = filled_new();
        let first = form.restart_try();
        form.try_request(first).unwrap();
        let second = form.restart_try();

        form.try_answered(first, Ok(TryResult::default()));
        assert_eq!(form.live_try.status, TryStatus::Waiting);
        form.try_request(second).unwrap();
        form.try_answered(second, Ok(TryResult::default()));

        assert_eq!(form.live_try.status, TryStatus::Ready(TryResult::default()));
        form.try_answered(first, Err("late".to_owned()));
        assert_eq!(form.live_try.status, TryStatus::Ready(TryResult::default()));
    }

    #[test]
    fn conditions_that_cannot_be_tried_send_nothing() {
        let mut empty_row = RuleForm::new();
        let generation = empty_row.restart_try();
        assert_eq!(empty_row.try_request(generation), None);
        assert_eq!(empty_row.live_try.status, TryStatus::Idle);

        let mut broken_regex = filled_new();
        broken_regex.update(RuleFormMessage::Condition(ConditionEdit::Kind(
            0,
            ConditionKind::Regex,
        )));
        broken_regex.update(RuleFormMessage::Condition(ConditionEdit::Value(
            0,
            "(".to_owned(),
        )));
        let broken_generation = broken_regex.restart_try();
        assert_eq!(broken_regex.try_request(broken_generation), None);
        assert_eq!(broken_regex.live_try.status, TryStatus::Idle);
    }

    #[test]
    fn an_invalid_pattern_is_still_tried() {
        let mut form = filled_new();
        form.update(RuleFormMessage::Pattern("{foo}".to_owned()));
        let generation = form.restart_try();

        assert!(form.try_request(generation).is_some());
    }

    #[test]
    fn the_try_of_an_edit_replaces_the_stored_rule() {
        let rule = stored_rule();
        let mut form = RuleForm::edit(&rule);
        let generation = form.restart_try();

        let request = form.try_request(generation).unwrap();

        assert_eq!(request.replaces, Some(rule.id));
        assert_eq!(request.spec.extract_date, rule.spec.extract_date);
    }
}
