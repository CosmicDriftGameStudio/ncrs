//! The form of a rule: a new one, or an existing one being changed. Nothing is
//! stored before Save. No widgets, the view is `ui::rules`.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use ncrs_index::{
    kind_tag_word, stored_kind, tag_name, Condition, ConditionKind, KindLanguage, Rule,
    RuleConditions, RuleId, RuleOrigin, RuleSpec,
};

use crate::condition_editor::{ConditionEditor, ConditionProblem, NEW_CONDITION_KINDS};
use crate::filing_pieces::TargetEditor;
use crate::messages::RuleFormMessage;
use crate::rules_work::{RuleTry, TryResult};

const NEW_RULE_PATTERN: &str = "{year}/{sender}/";

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
    pub name: String,
    pub conditions: ConditionEditor,
    /// The kind the rule sets; the form does not show it, so a stored one
    /// stays.
    pub kept_kind: Option<String>,
    /// The tags the rule sets.
    pub tags: BTreeSet<String>,
    pub tag_input: String,
    pub filing: TargetEditor,
    pub saving: bool,
    /// The index refused the rule; the technical detail stays English.
    pub failure: Option<String>,
    /// The documents the rule as it stands in the form would match.
    pub live_try: TryState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleFormProblems {
    /// One entry per condition row.
    pub conditions: Vec<Option<ConditionProblem>>,
    /// A rule without a condition would match every document.
    pub no_condition: bool,
    /// The target has no folder and no file name.
    pub no_target: bool,
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
        Self::of(
            RuleFormTarget::New,
            RuleConditions::all_of(vec![Condition::new(ConditionKind::Tag, "")]),
            NEW_RULE_PATTERN,
        )
    }

    pub fn edit(rule: &Rule) -> Self {
        Self::of_rule(rule, RuleFormTarget::Edit(rule.clone()))
    }

    pub fn from_suggestion(rule: &Rule) -> Self {
        Self::of_rule(rule, RuleFormTarget::FromSuggestion(rule.clone()))
    }

    /// The conditions are the tags the kind and the sender of the document
    /// give; the target is the pattern of the review.
    pub fn from_document(prefill: RulePrefill, kind_language: KindLanguage) -> Self {
        let tag_conditions: Vec<Condition> = [
            kind_tag_word(&tag_name(&prefill.kind), kind_language),
            tag_name(&prefill.sender),
        ]
        .into_iter()
        .filter(|tag| !tag.is_empty())
        .map(|tag| Condition::new(ConditionKind::Tag, tag))
        .collect();
        let conditions = if tag_conditions.is_empty() {
            vec![Condition::new(ConditionKind::Tag, "")]
        } else {
            tag_conditions
        };
        let pattern = if prefill.pattern.trim().is_empty() {
            NEW_RULE_PATTERN
        } else {
            prefill.pattern.trim()
        };
        let mut form = Self::of(
            RuleFormTarget::FromDocument,
            RuleConditions::all_of(conditions),
            pattern,
        );
        form.kept_kind = Some(stored_kind(&prefill.kind)).filter(|kind| !kind.is_empty());
        form.tags = prefill.tags;
        form
    }

    fn of(target: RuleFormTarget, conditions: RuleConditions, pattern: &str) -> Self {
        Self {
            target,
            name: String::new(),
            conditions: ConditionEditor::new(conditions, &NEW_CONDITION_KINDS, true),
            kept_kind: None,
            tags: BTreeSet::new(),
            tag_input: String::new(),
            filing: TargetEditor::new(pattern),
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
        let mut form = Self::of(target, spec.conditions.clone(), &spec.filing_pattern);
        form.name = spec.name.clone().unwrap_or_default();
        form.kept_kind = spec.kind.clone();
        form.tags = spec.tags.clone();
        form
    }

    /// Applies the messages that change a field; the others are the page's.
    pub fn update(&mut self, message: RuleFormMessage) {
        match message {
            RuleFormMessage::Name(name) => self.name = name,
            RuleFormMessage::Condition(edit) => self.conditions.apply(edit),
            RuleFormMessage::Target(edit) => {
                self.filing.apply(edit);
            }
            RuleFormMessage::TagInput(typed) => self.tag_input = typed,
            RuleFormMessage::AddTag => {
                let tag = tag_name(&self.tag_input);
                self.tag_input.clear();
                if !tag.is_empty() {
                    self.tags.insert(tag);
                }
            }
            RuleFormMessage::RemoveTag(tag) => {
                self.tags.remove(&tag);
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
            name: Some(self.name.trim().to_owned()).filter(|name| !name.is_empty()),
            conditions: self.conditions.conditions(),
            extract_number: kept.and_then(|spec| spec.extract_number.clone()),
            extract_date: kept.and_then(|spec| spec.extract_date.clone()),
            extract_amount: kept.and_then(|spec| spec.extract_amount.clone()),
            kind: self.kept_kind.clone(),
            tags: self.tags.clone(),
            filing_pattern: self.filing.pattern(),
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
            no_target: self.save_was_tried() && !self.filing.is_usable(),
        }
    }

    pub fn is_valid(&self) -> bool {
        self.conditions.is_valid(false) && self.filing.is_usable()
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
    use crate::filing_pieces::{FolderField, TargetEdit};
    use crate::rules_page::tests::rule_id;

    fn stored_rule() -> Rule {
        Rule {
            id: rule_id(1),
            position: 1,
            origin: RuleOrigin::Suggestion,
            spec: RuleSpec {
                name: Some("Rent".to_owned()),
                conditions: RuleConditions::any_of(vec![
                    Condition::new(ConditionKind::Word, "rent"),
                    Condition::not(ConditionKind::Sender, "Agency B"),
                ]),
                extract_number: None,
                extract_date: Some(r"\d{4}-\d{2}-\d{2}".to_owned()),
                extract_amount: None,
                kind: Some("Invoice".to_owned()),
                tags: BTreeSet::from(["tax".to_owned(), "office".to_owned()]),
                filing_pattern: "{year}/{sender}/".to_owned(),
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
        form
    }

    fn clear_target(form: &mut RuleForm) {
        while !form.filing.target.folders.is_empty() {
            form.update(RuleFormMessage::Target(TargetEdit::Remove(0)));
        }
    }

    #[test]
    fn a_new_form_starts_with_one_empty_tag_row_and_the_usual_target() {
        let form = RuleForm::new();

        assert_eq!(form.conditions.rows.len(), 1);
        assert_eq!(form.conditions.rows[0].kind, ConditionKind::Tag);
        assert_eq!(form.conditions.rows[0].value, "");
        assert_eq!(form.filing.pattern(), "{year}/{sender}/");
        assert!(!form.is_valid());
    }

    #[test]
    fn an_edit_form_starts_from_the_rule() {
        let form = RuleForm::edit(&stored_rule());

        assert_eq!(form.name, "Rent");
        assert_eq!(
            form.tags,
            BTreeSet::from(["office".to_owned(), "tax".to_owned()])
        );
        assert_eq!(form.conditions.rows.len(), 2);
        assert!(form.is_valid());
    }

    #[test]
    fn nothing_is_reported_before_the_first_save_attempt() {
        let mut form = RuleForm::new();
        clear_target(&mut form);
        assert!(!form.problems().no_target);

        assert_eq!(form.save_request(), None);

        let problems = form.problems();
        assert!(problems.no_target);
        assert_eq!(problems.conditions, vec![Some(ConditionProblem::Empty)]);
    }

    #[test]
    fn a_rule_without_a_condition_cannot_be_saved() {
        let mut form = filled_new();
        form.update(RuleFormMessage::Condition(ConditionEdit::Remove(0)));

        assert_eq!(form.save_request(), None);
        assert!(form.problems().no_condition);
    }

    #[test]
    fn a_rule_without_a_target_cannot_be_saved() {
        let mut form = filled_new();
        clear_target(&mut form);

        assert_eq!(form.save_request(), None);
        assert!(form.problems().no_target);
    }

    #[test]
    fn tags_are_added_normalised_and_removed() {
        let mut form = RuleForm::new();

        form.update(RuleFormMessage::TagInput("  Steuer ".to_owned()));
        form.update(RuleFormMessage::AddTag);
        form.update(RuleFormMessage::TagInput("steuer".to_owned()));
        form.update(RuleFormMessage::AddTag);
        form.update(RuleFormMessage::TagInput("  ".to_owned()));
        form.update(RuleFormMessage::AddTag);
        form.update(RuleFormMessage::TagInput("Büro".to_owned()));
        form.update(RuleFormMessage::AddTag);

        assert_eq!(form.tags.len(), 2);
        assert_eq!(form.tag_input, "");
        let first = form.tags.iter().next().cloned().unwrap();
        form.update(RuleFormMessage::RemoveTag(first));
        assert_eq!(form.tags.len(), 1);
    }

    #[test]
    fn a_new_form_saves_a_confirmed_enabled_rule_once() {
        let mut form = filled_new();
        form.update(RuleFormMessage::Name("  Telekom bills ".to_owned()));

        let first = form.save_request().unwrap();
        let second = form.save_request();

        assert_eq!(second, None);
        let RuleSave::Create { spec, origin } = first else {
            panic!("not a create");
        };
        assert_eq!(origin, RuleOrigin::Hand);
        assert_eq!(
            spec.conditions,
            RuleConditions::all_of(vec![Condition::new(ConditionKind::Tag, "Telekom")])
        );
        assert_eq!(spec.name.as_deref(), Some("Telekom bills"));
        assert_eq!(spec.kind, None);
        assert_eq!(spec.filing_pattern, "{year}/{sender}/");
        assert!(spec.confirmed && spec.enabled);
        assert!(spec.extract_date.is_none() && spec.extract_number.is_none());
    }

    #[test]
    fn a_blank_name_is_no_name() {
        let mut form = filled_new();
        form.update(RuleFormMessage::Name("   ".to_owned()));

        let RuleSave::Create { spec, .. } = form.save_request().unwrap() else {
            panic!("not a create");
        };

        assert_eq!(spec.name, None);
    }

    #[test]
    fn an_edit_form_keeps_what_it_does_not_show() {
        let rule = stored_rule();
        let mut form = RuleForm::edit(&rule);
        form.update(RuleFormMessage::Target(TargetEdit::AddField(
            FolderField::Month,
        )));
        form.update(RuleFormMessage::TagInput("home".to_owned()));
        form.update(RuleFormMessage::AddTag);

        let RuleSave::Update(id, spec) = form.save_request().unwrap() else {
            panic!("not an update");
        };

        assert_eq!(id, rule.id);
        assert_eq!(spec.filing_pattern, "{year}/{sender}/{month}/");
        assert_eq!(spec.kind.as_deref(), Some("Invoice"));
        assert_eq!(spec.tags.len(), 3);
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
            sender: "Company A".to_owned(),
            kind: "Invoice".to_owned(),
            tags: BTreeSet::from(["tax".to_owned()]),
            pattern: "Firma A/{year}/{sender}/".to_owned(),
        }
    }

    #[test]
    fn a_form_from_a_document_asks_for_the_tags_of_its_kind_and_sender() {
        let form = RuleForm::from_document(prefill(), KindLanguage::English);

        assert_eq!(form.target, RuleFormTarget::FromDocument);
        assert_eq!(
            form.conditions.conditions(),
            RuleConditions::all_of(vec![
                Condition::new(ConditionKind::Tag, "invoice"),
                Condition::new(ConditionKind::Tag, "company-a"),
            ])
        );
        assert_eq!(form.tags, BTreeSet::from(["tax".to_owned()]));
        assert_eq!(form.filing.pattern(), "Firma A/{year}/{sender}/");
        assert!(form.is_valid());
    }

    #[test]
    fn a_form_from_a_document_words_the_kind_tag_in_the_language_and_keeps_the_id() {
        let mut form = RuleForm::from_document(prefill(), KindLanguage::German);

        assert_eq!(
            form.conditions.conditions(),
            RuleConditions::all_of(vec![
                Condition::new(ConditionKind::Tag, "rechnung"),
                Condition::new(ConditionKind::Tag, "company-a"),
            ])
        );
        let RuleSave::Create { spec, .. } = form.save_request().unwrap() else {
            panic!("not a create");
        };
        assert_eq!(spec.kind.as_deref(), Some("invoice"));
    }

    #[test]
    fn a_form_from_a_document_without_a_pattern_takes_the_usual_one() {
        let form = RuleForm::from_document(
            RulePrefill {
                pattern: "  ".to_owned(),
                sender: String::new(),
                kind: String::new(),
                ..prefill()
            },
            KindLanguage::English,
        );

        assert_eq!(form.filing.pattern(), "{year}/{sender}/");
        assert_eq!(form.conditions.rows.len(), 1);
        assert_eq!(form.conditions.rows[0].value, "");
        assert!(!form.is_valid());
    }

    #[test]
    fn a_form_from_a_document_saves_a_rule_with_the_origin_filing_and_its_kind() {
        let mut form = RuleForm::from_document(prefill(), KindLanguage::English);

        let RuleSave::Create { spec, origin } = form.save_request().unwrap() else {
            panic!("not a create");
        };

        assert_eq!(origin, RuleOrigin::Filing);
        assert!(spec.confirmed && spec.enabled);
        assert_eq!(spec.kind.as_deref(), Some("invoice"));
        assert_eq!(spec.tags, BTreeSet::from(["tax".to_owned()]));
    }

    #[test]
    fn a_form_from_a_suggestion_confirms_it_and_keeps_what_it_does_not_show() {
        let rule = stored_rule();
        let mut form = RuleForm::from_suggestion(&rule);
        assert_eq!(form.target, RuleFormTarget::FromSuggestion(rule.clone()));
        form.update(RuleFormMessage::Target(TargetEdit::Remove(1)));

        let RuleSave::ConfirmSuggestion(id, spec) = form.save_request().unwrap() else {
            panic!("not a confirmation");
        };

        assert_eq!(id, rule.id);
        assert_eq!(spec.filing_pattern, "{year}/");
        assert_eq!(spec.extract_date, rule.spec.extract_date);
        assert_eq!(spec.conditions, rule.spec.conditions);
    }

    #[test]
    fn an_edit_waits_and_only_the_latest_generation_sends_a_request() {
        let mut form = filled_new();
        let first = form.restart_try();
        assert_eq!(form.live_try.status, TryStatus::Waiting);
        form.update(RuleFormMessage::Target(TargetEdit::Remove(1)));
        let second = form.restart_try();

        assert_ne!(first, second);
        assert_eq!(form.try_request(first), None);
        assert_eq!(form.live_try.status, TryStatus::Waiting);
        let request = form.try_request(second).unwrap();
        assert_eq!(form.live_try.status, TryStatus::Running);
        assert_eq!(request.replaces, None);
        assert_eq!(request.spec.filing_pattern, "{year}/");
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
    fn the_try_of_an_edit_replaces_the_stored_rule() {
        let rule = stored_rule();
        let mut form = RuleForm::edit(&rule);
        let generation = form.restart_try();

        let request = form.try_request(generation).unwrap();

        assert_eq!(request.replaces, Some(rule.id));
        assert_eq!(request.spec.extract_date, rule.spec.extract_date);
    }
}
