//! The form of a rule: a new one, or an existing one being changed. Nothing is
//! stored before Save; the conditions are edited with the editor the tax
//! contexts use too. No widgets, the view is `ui::rules`.

use std::collections::BTreeSet;

use ncrs_index::{
    Condition, ConditionKind, DocumentFields, Rule, RuleConditions, RuleId, RuleOrigin, RuleSpec,
    TaxContext,
};

use crate::condition_editor::{ConditionEditor, ConditionProblem, ALL_CONDITION_KINDS};
use crate::messages::RuleFormMessage;
use crate::review_work::{parse_tags, tags_text};
use crate::tax_contexts::{rendered_example, sample_fields};

const NEW_RULE_PATTERN: &str = "{year}/{sender}";
const EXAMPLE_KIND: &str = "invoice";
const EXAMPLE_TAX_CONTEXT: &str = "company";

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
    pub tax_context: Option<TaxContext>,
    pub tags: BTreeSet<String>,
    pub pattern: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuleForm {
    pub target: RuleFormTarget,
    pub conditions: ConditionEditor,
    pub kind: String,
    /// `None`: the conditions of the tax contexts decide.
    pub tax_context: Option<TaxContext>,
    /// Comma separated, like the review list.
    pub tags: String,
    pub pattern: String,
    pub saving: bool,
    /// The index refused the rule; the technical detail stays English.
    pub failure: Option<String>,
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
            tax_context: None,
            tags: String::new(),
            pattern: NEW_RULE_PATTERN.to_owned(),
            saving: false,
            failure: None,
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
            tax_context: prefill.tax_context,
            tags: tags_text(&prefill.tags),
            pattern,
            saving: false,
            failure: None,
        }
    }

    fn of_rule(rule: &Rule, target: RuleFormTarget) -> Self {
        let spec = &rule.spec;
        Self {
            target,
            conditions: ConditionEditor::new(spec.conditions.clone(), &ALL_CONDITION_KINDS, true),
            kind: spec.kind.clone(),
            tax_context: spec.tax_context.clone(),
            tags: tags_text(&spec.tags),
            pattern: spec.filing_pattern.clone(),
            saving: false,
            failure: None,
        }
    }

    /// Applies the messages that change a field; the others are the page's.
    pub fn update(&mut self, message: RuleFormMessage) {
        match message {
            RuleFormMessage::Condition(edit) => self.conditions.apply(edit),
            RuleFormMessage::Kind(kind) => self.kind = kind,
            RuleFormMessage::TaxContext(tax_context) => self.tax_context = tax_context,
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

    /// The path the pattern gives for a sample document, in the kind and tax
    /// context of the form.
    pub fn example_path(&self) -> Option<String> {
        let kind = self.kind.trim();
        let fields = DocumentFields {
            kind: Some(if kind.is_empty() { EXAMPLE_KIND } else { kind }.to_owned()),
            tax_context: Some(
                self.tax_context
                    .clone()
                    .unwrap_or_else(|| TaxContext::named(EXAMPLE_TAX_CONTEXT)),
            ),
            ..sample_fields()
        };
        rendered_example(self.pattern.trim(), &fields)
    }

    /// The write to make, once; the answer comes to the page's `saved`.
    pub fn save_request(&mut self) -> Option<RuleSave> {
        self.conditions.mark_save_attempt();
        if self.saving || !self.is_valid() {
            return None;
        }
        self.saving = true;
        self.failure = None;
        let conditions = self.conditions.conditions();
        let kind = self.kind.trim().to_owned();
        let tax_context = self.tax_context.clone();
        let tags = parse_tags(&self.tags);
        let filing_pattern = self.pattern.trim().to_owned();
        let new_rule = || RuleSpec {
            conditions: conditions.clone(),
            extract_number: None,
            extract_date: None,
            extract_amount: None,
            kind: kind.clone(),
            tax_context: tax_context.clone(),
            tags: tags.clone(),
            filing_pattern: filing_pattern.clone(),
            confirmed: true,
            enabled: true,
        };
        let changed = |rule: &Rule| RuleSpec {
            conditions: conditions.clone(),
            kind: kind.clone(),
            tax_context: tax_context.clone(),
            tags: tags.clone(),
            filing_pattern: filing_pattern.clone(),
            ..rule.spec.clone()
        };
        Some(match &self.target {
            RuleFormTarget::New => RuleSave::Create {
                spec: new_rule(),
                origin: RuleOrigin::Hand,
            },
            RuleFormTarget::FromDocument => RuleSave::Create {
                spec: new_rule(),
                origin: RuleOrigin::Filing,
            },
            RuleFormTarget::Edit(rule) => RuleSave::Update(rule.id, changed(rule)),
            RuleFormTarget::FromSuggestion(rule) => {
                RuleSave::ConfirmSuggestion(rule.id, changed(rule))
            }
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
                tax_context: Some(TaxContext::None),
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
        assert_eq!(form.tax_context, None);
        assert!(!form.is_valid());
    }

    #[test]
    fn an_edit_form_starts_from_the_rule() {
        let form = RuleForm::edit(&stored_rule());

        assert_eq!(form.kind, "Invoice");
        assert_eq!(form.tags, "office, tax");
        assert_eq!(form.tax_context, Some(TaxContext::None));
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
    fn the_example_uses_the_sample_document_and_the_form_kind_and_context() {
        let mut form = RuleForm::new();
        assert_eq!(form.example_path().as_deref(), Some("2024/Telekom"));

        form.update(RuleFormMessage::Pattern(
            "{tax_context}/{kind}/{year}/{number}".to_owned(),
        ));
        assert_eq!(
            form.example_path().as_deref(),
            Some("company/invoice/2024/4711")
        );
        form.update(RuleFormMessage::Kind("Receipt".to_owned()));
        form.update(RuleFormMessage::TaxContext(Some(TaxContext::named(
            "private",
        ))));
        assert_eq!(
            form.example_path().as_deref(),
            Some("private/Receipt/2024/4711")
        );
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
        assert_eq!(spec.tax_context, None);
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
            tax_context: Some(TaxContext::named("company")),
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
        assert_eq!(form.tax_context, Some(TaxContext::named("company")));
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
}
