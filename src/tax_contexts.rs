//! What the tax contexts section of the Hermes settings remembers: the list
//! read from the index and the step the user is in. Nothing is written before
//! a step is confirmed; leaving a step changes nothing.

use chrono::NaiveDate;
use ncrs_index::{
    Combinator, Condition, ConditionKind, DocumentFields, RuleConditions, TaxContext,
    TaxContextEntry, TaxContextId, TaxContextUsage,
};

use crate::tax_context_work::{ContextsResult, TaxContextFailure};

/// The pattern the example path is made with, so the user sees how the short
/// name is sanitised and where it lands.
const SAMPLE_PATTERN: &str = "{tax_context}/{year}/{sender}";
const SAMPLE_SENDER: &str = "Telekom";

pub fn example_path(short_name: &str) -> Option<String> {
    let fields = DocumentFields {
        tax_context: Some(TaxContext::named(short_name.trim())),
        sender: Some(SAMPLE_SENDER.to_owned()),
        date: NaiveDate::from_ymd_opt(2024, 3, 15),
        ..DocumentFields::default()
    };
    let path = ncrs_intake::render_pattern(SAMPLE_PATTERN, &fields)?;
    Some(
        path.components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

#[derive(Debug, Clone, PartialEq)]
pub enum ContextList {
    Loading,
    Ready(Vec<TaxContextEntry>),
    Failed(TaxContextFailure),
}

/// The fields of the add and rename form.
#[derive(Debug, Clone, PartialEq)]
pub struct EditForm {
    /// The context being renamed; `None` while a new one is added.
    pub target: Option<TaxContextId>,
    /// The name the context had when the form opened, for the heading.
    pub previous_name: String,
    pub name: String,
    pub short_name: String,
    pub failure: Option<TaxContextFailure>,
    pub saving: bool,
}

/// The delete question. What uses the context is looked up first, so the
/// question can name it.
#[derive(Debug, Clone, PartialEq)]
pub struct DeleteQuestion {
    pub entry: TaxContextEntry,
    pub usage: Option<TaxContextUsage>,
    /// Where documents, rules and saved queries go.
    pub move_to: TaxContext,
    pub failure: Option<TaxContextFailure>,
    pub deleting: bool,
}

/// The conditions that assign documents to a context. They are read first, so
/// nothing is saved over conditions that were never seen.
#[derive(Debug, Clone, PartialEq)]
pub struct ConditionForm {
    pub entry: TaxContextEntry,
    pub conditions: Option<RuleConditions>,
    pub new_kind: ConditionKind,
    pub new_value: String,
    pub failure: Option<TaxContextFailure>,
    pub saving: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Idle,
    Editing(EditForm),
    Deleting(DeleteQuestion),
    Conditions(ConditionForm),
}

/// What the user may pick for a new condition; the regex, address and folder
/// kinds stay with the rules.
pub const NEW_CONDITION_KINDS: [ConditionKind; 6] = [
    ConditionKind::Recipient,
    ConditionKind::Sender,
    ConditionKind::Word,
    ConditionKind::Substring,
    ConditionKind::Iban,
    ConditionKind::TaxId,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConditionsRequest {
    pub id: TaxContextId,
    pub conditions: RuleConditions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveRequest {
    pub target: Option<TaxContextId>,
    pub name: String,
    pub short_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteRequest {
    pub id: TaxContextId,
    pub move_to: TaxContext,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaxContextsState {
    pub list: ContextList,
    pub step: Step,
}

impl Default for TaxContextsState {
    fn default() -> Self {
        Self {
            list: ContextList::Loading,
            step: Step::Idle,
        }
    }
}

impl TaxContextsState {
    pub fn entries(&self) -> &[TaxContextEntry] {
        match &self.list {
            ContextList::Ready(entries) => entries,
            ContextList::Loading | ContextList::Failed(_) => &[],
        }
    }

    fn entry(&self, id: TaxContextId) -> Option<&TaxContextEntry> {
        self.entries().iter().find(|entry| entry.id == id)
    }

    pub fn loaded(&mut self, result: ContextsResult) {
        self.list = match result {
            Ok(entries) => ContextList::Ready(entries),
            Err(failure) => ContextList::Failed(failure),
        };
    }

    pub fn begin_add(&mut self) {
        if matches!(self.list, ContextList::Ready(_)) {
            self.step = Step::Editing(EditForm {
                target: None,
                previous_name: String::new(),
                name: String::new(),
                short_name: String::new(),
                failure: None,
                saving: false,
            });
        }
    }

    pub fn begin_rename(&mut self, id: TaxContextId) {
        let Some(entry) = self.entry(id) else {
            return;
        };
        self.step = Step::Editing(EditForm {
            target: Some(id),
            previous_name: entry.name.clone(),
            name: entry.name.clone(),
            short_name: entry.short_name.clone(),
            failure: None,
            saving: false,
        });
    }

    pub fn edit_name(&mut self, name: String) {
        if let Step::Editing(form) = &mut self.step {
            form.name = name;
            form.failure = None;
        }
    }

    pub fn edit_short_name(&mut self, short_name: String) {
        if let Step::Editing(form) = &mut self.step {
            form.short_name = short_name;
            form.failure = None;
        }
    }

    /// The request to store the form, once; the answer comes to [`Self::saved`].
    pub fn save_request(&mut self) -> Option<SaveRequest> {
        let Step::Editing(form) = &mut self.step else {
            return None;
        };
        if form.saving {
            return None;
        }
        form.saving = true;
        Some(SaveRequest {
            target: form.target,
            name: form.name.clone(),
            short_name: form.short_name.clone(),
        })
    }

    pub fn saved(&mut self, result: ContextsResult) {
        match result {
            Ok(entries) => {
                self.list = ContextList::Ready(entries);
                if matches!(self.step, Step::Editing(_)) {
                    self.step = Step::Idle;
                }
            }
            Err(failure) => {
                if let Step::Editing(form) = &mut self.step {
                    form.saving = false;
                    form.failure = Some(failure);
                }
            }
        }
    }

    pub fn begin_delete(&mut self, id: TaxContextId) {
        let Some(entry) = self.entry(id).cloned() else {
            return;
        };
        self.step = Step::Deleting(DeleteQuestion {
            entry,
            usage: None,
            move_to: TaxContext::None,
            failure: None,
            deleting: false,
        });
    }

    pub fn usage_loaded(
        &mut self,
        id: TaxContextId,
        result: Result<TaxContextUsage, TaxContextFailure>,
    ) {
        let Step::Deleting(question) = &mut self.step else {
            return;
        };
        if question.entry.id != id {
            return;
        }
        match result {
            Ok(usage) => question.usage = Some(usage),
            Err(failure) => question.failure = Some(failure),
        }
    }

    /// The contexts the uses of the one being deleted may move to.
    pub fn move_targets(&self) -> Vec<&TaxContextEntry> {
        let Step::Deleting(question) = &self.step else {
            return Vec::new();
        };
        self.entries()
            .iter()
            .filter(|entry| entry.id != question.entry.id)
            .collect()
    }

    pub fn choose_move_to(&mut self, choice: TaxContext) {
        let allowed = match &choice {
            TaxContext::None => true,
            TaxContext::Named(short_name) => self
                .move_targets()
                .iter()
                .any(|entry| entry.short_name == *short_name),
        };
        if let (true, Step::Deleting(question)) = (allowed, &mut self.step) {
            question.move_to = choice;
        }
    }

    /// The request to delete, once and only after it is known what uses the
    /// context; the answer comes to [`Self::deleted`].
    pub fn delete_request(&mut self) -> Option<DeleteRequest> {
        let Step::Deleting(question) = &mut self.step else {
            return None;
        };
        if question.deleting || question.usage.is_none() {
            return None;
        }
        question.deleting = true;
        Some(DeleteRequest {
            id: question.entry.id,
            move_to: question.move_to.clone(),
        })
    }

    pub fn deleted(&mut self, result: ContextsResult) {
        match result {
            Ok(entries) => {
                self.list = ContextList::Ready(entries);
                if matches!(self.step, Step::Deleting(_)) {
                    self.step = Step::Idle;
                }
            }
            Err(failure) => {
                if let Step::Deleting(question) = &mut self.step {
                    question.deleting = false;
                    question.failure = Some(failure);
                }
            }
        }
    }

    pub fn cancel(&mut self) {
        self.step = Step::Idle;
    }

    pub fn begin_conditions(&mut self, id: TaxContextId) {
        let Some(entry) = self.entry(id).cloned() else {
            return;
        };
        self.step = Step::Conditions(ConditionForm {
            entry,
            conditions: None,
            new_kind: ConditionKind::Recipient,
            new_value: String::new(),
            failure: None,
            saving: false,
        });
    }

    pub fn conditions_loaded(
        &mut self,
        id: TaxContextId,
        result: Result<RuleConditions, TaxContextFailure>,
    ) {
        let Step::Conditions(form) = &mut self.step else {
            return;
        };
        if form.entry.id != id {
            return;
        }
        match result {
            Ok(conditions) => form.conditions = Some(conditions),
            Err(failure) => form.failure = Some(failure),
        }
    }

    /// The form of the conditions, once they are read.
    fn loaded_conditions_form(&mut self) -> Option<&mut ConditionForm> {
        match &mut self.step {
            Step::Conditions(form) if form.conditions.is_some() => Some(form),
            _ => None,
        }
    }

    pub fn choose_combinator(&mut self, combinator: Combinator) {
        if let Some(conditions) = self
            .loaded_conditions_form()
            .and_then(|form| form.conditions.as_mut())
        {
            conditions.combinator = combinator;
        }
    }

    pub fn choose_new_condition_kind(&mut self, kind: ConditionKind) {
        if let Step::Conditions(form) = &mut self.step {
            form.new_kind = kind;
        }
    }

    pub fn edit_new_condition_value(&mut self, value: String) {
        if let Step::Conditions(form) = &mut self.step {
            form.new_value = value;
        }
    }

    /// Adds the typed value as a condition of the chosen kind; a blank value
    /// adds nothing.
    pub fn add_condition(&mut self) {
        let Some(ConditionForm {
            conditions: Some(conditions),
            new_kind,
            new_value,
            failure,
            ..
        }) = self.loaded_conditions_form()
        else {
            return;
        };
        let value = new_value.trim();
        if value.is_empty() {
            return;
        }
        conditions.list.push(Condition::new(*new_kind, value));
        new_value.clear();
        *failure = None;
    }

    pub fn remove_condition(&mut self, position: usize) {
        if let Some(conditions) = self
            .loaded_conditions_form()
            .and_then(|form| form.conditions.as_mut())
        {
            if position < conditions.list.len() {
                conditions.list.remove(position);
            }
        }
    }

    /// The request to store the conditions, once; the answer comes to
    /// [`Self::conditions_saved`].
    pub fn conditions_request(&mut self) -> Option<ConditionsRequest> {
        let form = self.loaded_conditions_form()?;
        if form.saving {
            return None;
        }
        let conditions = form.conditions.clone()?;
        form.saving = true;
        Some(ConditionsRequest {
            id: form.entry.id,
            conditions,
        })
    }

    pub fn conditions_saved(&mut self, result: ContextsResult) {
        match result {
            Ok(entries) => {
                self.list = ContextList::Ready(entries);
                if matches!(self.step, Step::Conditions(_)) {
                    self.step = Step::Idle;
                }
            }
            Err(failure) => {
                if let Step::Conditions(form) = &mut self.step {
                    form.saving = false;
                    form.failure = Some(failure);
                }
            }
        }
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use ncrs_index::{Index, TaxContextProblem};

    use super::*;

    fn stored() -> (tempfile::TempDir, Index) {
        let dir = tempfile::TempDir::new().unwrap();
        let index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        (dir, index)
    }

    fn loaded_state(index: &Index) -> TaxContextsState {
        let mut state = TaxContextsState::default();
        state.loaded(Ok(index.tax_contexts().unwrap()));
        state
    }

    fn id_of(state: &TaxContextsState, short_name: &str) -> TaxContextId {
        state
            .entries()
            .iter()
            .find(|entry| entry.short_name == short_name)
            .unwrap()
            .id
    }

    #[test]
    fn the_example_path_shows_the_short_name_as_it_is_filed() {
        assert_eq!(
            example_path("company").as_deref(),
            Some("company/2024/Telekom")
        );
        assert_eq!(
            example_path("  Firma A ").as_deref(),
            Some("Firma A/2024/Telekom")
        );
        assert_eq!(example_path("   "), None);
    }

    #[test]
    fn nothing_can_be_added_before_the_list_is_read() {
        let mut state = TaxContextsState::default();

        state.begin_add();

        assert_eq!(state.step, Step::Idle);
    }

    #[test]
    fn a_form_is_saved_once_and_a_refusal_keeps_it_open_with_the_reason() {
        let (_dir, index) = stored();
        let mut state = loaded_state(&index);
        state.begin_add();
        state.edit_name("Firma B".to_owned());
        state.edit_short_name("company-b".to_owned());

        let first = state.save_request().unwrap();
        let second = state.save_request();
        state.saved(Err(TaxContextFailure::Refused(
            TaxContextProblem::ShortNameTaken,
        )));

        assert_eq!(first.name, "Firma B");
        assert_eq!(first.target, None);
        assert_eq!(second, None);
        let Step::Editing(form) = &state.step else {
            panic!("the form closed");
        };
        assert_eq!(
            form.failure,
            Some(TaxContextFailure::Refused(
                TaxContextProblem::ShortNameTaken
            ))
        );
        assert!(state.save_request().is_some());
    }

    #[test]
    fn saving_closes_the_form_and_takes_over_the_new_list() {
        let (_dir, mut index) = stored();
        let mut state = loaded_state(&index);
        state.begin_add();
        index.create_tax_context("Firma B", "company-b").unwrap();

        state.saved(Ok(index.tax_contexts().unwrap()));

        assert_eq!(state.step, Step::Idle);
        assert_eq!(state.entries().len(), 3);
    }

    #[test]
    fn renaming_starts_from_the_stored_names_and_cancel_changes_nothing() {
        let (_dir, index) = stored();
        let mut state = loaded_state(&index);
        let before = state.clone();
        let company = id_of(&state, "company");

        state.begin_rename(company);
        state.edit_name("Firma A".to_owned());
        let Step::Editing(form) = &state.step else {
            panic!("no form");
        };
        assert_eq!(form.target, Some(company));
        assert_eq!(form.previous_name, "Firma");
        assert_eq!(form.short_name, "company");
        state.cancel();

        assert_eq!(state, before);
    }

    #[test]
    fn deleting_waits_for_the_usage_and_moves_to_no_context_unless_chosen() {
        let (_dir, index) = stored();
        let mut state = loaded_state(&index);
        let company = id_of(&state, "company");
        state.begin_delete(company);

        assert_eq!(state.delete_request(), None);
        state.usage_loaded(
            company,
            Ok(TaxContextUsage {
                documents: 12,
                rules: 2,
                saved_queries: 1,
            }),
        );
        state.choose_move_to(TaxContext::named("private"));
        let request = state.delete_request().unwrap();

        assert_eq!(request.id, company);
        assert_eq!(request.move_to, TaxContext::named("private"));
        assert_eq!(state.delete_request(), None);
    }

    #[test]
    fn a_delete_cannot_move_to_itself_or_to_a_context_that_is_not_stored() {
        let (_dir, index) = stored();
        let mut state = loaded_state(&index);
        let company = id_of(&state, "company");
        state.begin_delete(company);

        state.choose_move_to(TaxContext::named("company"));
        state.choose_move_to(TaxContext::named("missing"));

        let Step::Deleting(question) = &state.step else {
            panic!("no question");
        };
        assert_eq!(question.move_to, TaxContext::None);
        let targets: Vec<_> = state
            .move_targets()
            .iter()
            .map(|entry| entry.short_name.as_str())
            .collect();
        assert_eq!(targets, ["private"]);
    }

    #[test]
    fn cancelling_the_delete_question_keeps_every_context() {
        let (_dir, index) = stored();
        let mut state = loaded_state(&index);
        let before = state.entries().to_vec();
        state.begin_delete(id_of(&state, "private"));

        state.cancel();

        assert_eq!(state.step, Step::Idle);
        assert_eq!(state.entries(), before);
    }

    fn condition_form(state: &TaxContextsState) -> &ConditionForm {
        let Step::Conditions(form) = &state.step else {
            panic!("no condition form");
        };
        form
    }

    #[test]
    fn conditions_are_added_removed_and_saved_only_once_they_are_read() {
        let (_dir, index) = stored();
        let mut state = loaded_state(&index);
        let company = id_of(&state, "company");
        state.begin_conditions(company);

        state.edit_new_condition_value("Firma A GmbH".to_owned());
        state.add_condition();
        assert_eq!(state.conditions_request(), None);

        state.conditions_loaded(company, Ok(index.tax_context_conditions(company).unwrap()));
        state.edit_new_condition_value("  Firma A GmbH ".to_owned());
        state.add_condition();
        state.choose_new_condition_kind(ConditionKind::Iban);
        state.edit_new_condition_value("DE02 1203 0000 0000 2020 51".to_owned());
        state.add_condition();
        state.edit_new_condition_value("   ".to_owned());
        state.add_condition();
        state.choose_combinator(Combinator::AllOf);

        let form = condition_form(&state);
        assert_eq!(form.new_value, "   ");
        let request = state.conditions_request().unwrap();
        assert_eq!(request.id, company);
        assert_eq!(request.conditions.combinator, Combinator::AllOf);
        assert_eq!(
            request.conditions.list,
            vec![
                Condition::new(ConditionKind::Recipient, "Firma A GmbH"),
                Condition::new(ConditionKind::Iban, "DE02 1203 0000 0000 2020 51"),
            ]
        );
        assert_eq!(state.conditions_request(), None);
    }

    #[test]
    fn a_removed_condition_is_gone_from_what_is_saved() {
        let (_dir, mut index) = stored();
        let mut state = loaded_state(&index);
        let company = id_of(&state, "company");
        index
            .set_tax_context_conditions(
                company,
                &RuleConditions::any_of(vec![
                    Condition::new(ConditionKind::Recipient, "Firma A GmbH"),
                    Condition::new(ConditionKind::Word, "Firma A"),
                ]),
            )
            .unwrap();
        state.begin_conditions(company);
        state.conditions_loaded(company, Ok(index.tax_context_conditions(company).unwrap()));

        state.remove_condition(0);
        state.remove_condition(7);

        let request = state.conditions_request().unwrap();
        assert_eq!(
            request.conditions.list,
            vec![Condition::new(ConditionKind::Word, "Firma A")]
        );
    }

    #[test]
    fn saved_conditions_reach_the_index_and_a_failure_keeps_the_form_open() {
        let (_dir, mut index) = stored();
        let mut state = loaded_state(&index);
        let company = id_of(&state, "company");
        state.begin_conditions(company);
        state.conditions_loaded(company, Ok(index.tax_context_conditions(company).unwrap()));
        state.edit_new_condition_value("Firma A GmbH".to_owned());
        state.add_condition();
        let request = state.conditions_request().unwrap();

        state.conditions_saved(Err(TaxContextFailure::IndexNotOpen));
        assert_eq!(
            condition_form(&state).failure,
            Some(TaxContextFailure::IndexNotOpen)
        );
        assert!(state.conditions_request().is_some());

        index
            .set_tax_context_conditions(request.id, &request.conditions)
            .unwrap();
        state.conditions_saved(Ok(index.tax_contexts().unwrap()));

        assert_eq!(state.step, Step::Idle);
        assert_eq!(
            index.tax_context_conditions(company).unwrap().list,
            vec![Condition::new(ConditionKind::Recipient, "Firma A GmbH")]
        );
    }
}
