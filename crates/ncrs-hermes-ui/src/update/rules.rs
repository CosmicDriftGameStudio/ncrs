use std::ffi::OsStr;
use std::path::Path;
use std::sync::Arc;

use iced::widget::operation;
use iced::Task;
use ncrs_core::INDEX_NOT_OPEN;
use ncrs_filing::filing::FilingModule;
use ncrs_filing::rules_work::{
    try_in_target, DocumentForRule, RuleChange, RulesAccess, SortTry,
};
use ncrs_index::{DocumentId, TargetId};
use ncrs_intake::SortSource;
use ncrs_ui_kit::focus::release_focus;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::{Message, RuleFormMessage, RulesMessage};
use crate::rule_form::{FormTry, RuleForm, RulePrefill, RuleSave, TRY_DEBOUNCE};
use crate::rules_page::RulesPage;
use crate::state::Hermes;

/// The analysis starts with the view; the list appears when it is done.
pub fn update(
    hermes: &mut Hermes,
    message: RulesMessage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match message {
        RulesMessage::Loaded { generation, result } => {
            if generation != hermes.rules_generation {
                return Task::none();
            }
            if let Some(view) = hermes.rules.as_mut() {
                match result {
                    Ok(load) => view.loaded(load),
                    Err(detail) => view.failed(detail),
                }
            }
            schedule_rule_try(hermes)
        }
        RulesMessage::ToggleAt(index) => {
            let change = hermes
                .rules
                .as_mut()
                .and_then(|view| view.begin_toggle(index));
            run_rule_change(hermes, change)
        }
        RulesMessage::ReorderAt { index, delta } => {
            let change = hermes
                .rules
                .as_mut()
                .and_then(|view| view.begin_move(index, delta));
            run_rule_change(hermes, change)
        }
        RulesMessage::AskDelete(index) => {
            if let Some(view) = hermes.rules.as_mut() {
                view.ask_delete(index);
            }
            Task::none()
        }
        RulesMessage::CancelDelete => {
            if let Some(view) = hermes.rules.as_mut() {
                view.cancel_delete();
            }
            Task::none()
        }
        RulesMessage::ConfirmDelete => {
            let change = hermes.rules.as_mut().and_then(RulesPage::begin_delete);
            run_rule_change(hermes, change)
        }
        RulesMessage::Changed { generation, result } => {
            if generation != hermes.rules_generation {
                return Task::none();
            }
            match result {
                Ok(()) => {
                    hermes.watching.mark_all();
                    actions.push(Action::RulesChanged);
                    run_rules_load(hermes)
                }
                Err(detail) => {
                    if let Some(view) = hermes.rules.as_mut() {
                        view.failed(detail);
                    }
                    Task::none()
                }
            }
        }
        RulesMessage::NewIn(target) => {
            if let Some(view) = hermes.rules.as_mut() {
                view.begin_new_in(target);
            }
            Task::batch([focus_rule_form(hermes), schedule_rule_try(hermes)])
        }
        RulesMessage::Edit(index) => {
            if let Some(view) = hermes.rules.as_mut() {
                view.begin_edit(index);
            }
            Task::batch([focus_rule_form(hermes), schedule_rule_try(hermes)])
        }
        RulesMessage::Form(form_message) => update_rule_form(hermes, form_message, actions),
        RulesMessage::TryElapsed(generation) => run_rule_try(hermes, generation, ctx),
        RulesMessage::Tried { generation, result } => {
            if let Some(form) = hermes.rules.as_mut().and_then(RulesPage::form_mut) {
                form.try_answered(generation, result);
            }
            Task::none()
        }
        RulesMessage::DocumentLoaded(result) => {
            match result {
                Ok(Some(document)) => {
                    document_loaded(hermes, document, ctx);
                    Task::batch([focus_rule_form(hermes), schedule_rule_try(hermes)])
                }
                Ok(None) => Task::none(),
                Err(detail) => {
                    actions.push(Action::ShowError(detail));
                    Task::none()
                }
            }
        }
    }
}

/// The form that waited for its document fills in, unless the user typed.
fn document_loaded(hermes: &mut Hermes, document: DocumentForRule, ctx: &Ctx<'_>) {
    let Some(form) = hermes.rules.as_mut().and_then(RulesPage::form_mut) else {
        return;
    };
    if form.is_edited() {
        return;
    }
    let prefill = RulePrefill {
        sender: document.sender,
        kind: document.kind,
        tags: document.tags,
        pattern: String::new(),
    };
    let destination = form.destination.clone();
    let called = std::mem::take(&mut form.name);
    let mut filled = RuleForm::from_document(prefill, ctx.kinds, document.text.as_deref());
    if filled.name.is_empty() {
        filled.name = called;
    }
    *form = filled.in_target(destination);
}

fn focus_rule_form(hermes: &Hermes) -> Task<Message> {
    if hermes
        .rules
        .as_ref()
        .is_some_and(|view| view.form().is_some())
    {
        operation::focus(crate::ui::rules::NAME_FIELD_ID)
    } else {
        Task::none()
    }
}

/// A page that only carried a form is of no use once the form is gone.
fn drop_if_spent(hermes: &mut Hermes) {
    if hermes.rules.as_ref().is_some_and(RulesPage::is_spent) {
        hermes.rules = None;
    }
}

/// Closes the form without saving, if there is one; whether there was one.
fn cancel_rule_form(hermes: &mut Hermes) -> bool {
    let cancelled = hermes.rules.as_mut().is_some_and(RulesPage::cancel_form);
    drop_if_spent(hermes);
    cancelled
}

/// Closes the dialog that is up: the delete question or the form.
pub fn close_dialog(hermes: &mut Hermes) -> Task<Message> {
    if let Some(view) = hermes.rules.as_mut() {
        if !view.cancel_delete() {
            view.cancel_form();
        }
    }
    drop_if_spent(hermes);
    release_focus()
}

/// Opens the rule dialog for a new rule of `target` (the main one without),
/// over whatever is shown, with no settings page needed. With a document the
/// form starts from what the index knows of it once that is read; until then,
/// and where that names no sender, the rule is called like the file at `path`.
pub fn open_rule_form(
    hermes: &mut Hermes,
    target: Option<TargetId>,
    document: Option<DocumentId>,
    path: Option<&Path>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    let mut form = RuleForm::new();
    if let Some(stem) = path.and_then(Path::file_stem).and_then(OsStr::to_str) {
        stem.clone_into(&mut form.name);
    }
    let form = form.in_target(target.unwrap_or_else(TargetId::main));
    let read_document = match (document, hermes.filing.as_ref().and_then(FilingModule::rules_access)) {
        (Some(id), Some(access)) => Task::perform(
            tokio::task::spawn_blocking(move || access.document_for_rule(id)),
            |joined| {
                Message::Rules(RulesMessage::DocumentLoaded(
                    joined.unwrap_or_else(|join| Err(join.to_string())),
                ))
            },
        ),
        _ => Task::none(),
    };
    actions.push(Action::CloseContextMenu);
    match hermes.rules.as_mut() {
        Some(view) => view.open_form(form),
        None => {
            hermes.rules = Some(RulesPage::for_form(form, Arc::clone(&ctx.kinds.catalog)));
        }
    }
    Task::batch([read_document, focus_rule_form(hermes), schedule_rule_try(hermes)])
}

/// The form changed: its try waits for the form to rest.
fn schedule_rule_try(hermes: &mut Hermes) -> Task<Message> {
    let Some(form) = hermes.rules.as_mut().and_then(RulesPage::form_mut) else {
        return Task::none();
    };
    let generation = form.restart_try();
    Task::perform(
        async { tokio::time::sleep(TRY_DEBOUNCE).await },
        move |()| Message::Rules(RulesMessage::TryElapsed(generation)),
    )
}

/// The first source that files into `target`: the files the trial plans.
fn trial_source(hermes: &Hermes, target: &TargetId) -> Option<SortSource> {
    hermes
        .filing_config
        .sources
        .iter()
        .map(|setting| &setting.source)
        .find(|source| source.targets.contains(target))
        .cloned()
}

fn run_rule_try(hermes: &mut Hermes, generation: u64, ctx: &Ctx<'_>) -> Task<Message> {
    let Some(FormTry { target, rule }) = hermes
        .rules
        .as_mut()
        .and_then(RulesPage::form_mut)
        .and_then(|form| form.try_request(generation))
    else {
        return Task::none();
    };
    let source = trial_source(hermes, &target);
    let access = hermes
        .filing
        .as_ref()
        .and_then(|module| module.sorting_access(Arc::clone(&ctx.trash.0)));
    let immediate = match (&source, &access) {
        (None, _) => Some(Ok(SortTry::default())),
        (_, None) => Some(Err(INDEX_NOT_OPEN.to_owned())),
        _ => None,
    };
    if let Some(result) = immediate {
        if let Some(form) = hermes.rules.as_mut().and_then(RulesPage::form_mut) {
            form.try_answered(generation, result);
        }
        return Task::none();
    }
    let (Some(source), Some(access)) = (source, access) else {
        return Task::none();
    };
    Task::perform(
        tokio::task::spawn_blocking(move || try_in_target(&access, &source, &target, rule)),
        move |joined| {
            let result = joined.unwrap_or_else(|join| Err(join.to_string()));
            Message::Rules(RulesMessage::Tried { generation, result })
        },
    )
}

fn update_rule_form(
    hermes: &mut Hermes,
    message: RuleFormMessage,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    let Some(view) = hermes.rules.as_mut() else {
        return Task::none();
    };
    match message {
        RuleFormMessage::Save => {
            let save = view.begin_save();
            run_rule_save(hermes, save)
        }
        RuleFormMessage::Cancel => {
            cancel_rule_form(hermes);
            release_focus()
        }
        RuleFormMessage::Saved { generation, result } => {
            if generation != hermes.rules_generation {
                return Task::none();
            }
            match result {
                Ok(id) => {
                    view.saved(id);
                    hermes.watching.mark_all();
                    actions.push(Action::RulesChanged);
                    if hermes.rules.as_ref().is_some_and(RulesPage::is_spent) {
                        hermes.rules = None;
                        return release_focus();
                    }
                    run_rules_load(hermes)
                }
                Err(detail) => {
                    view.save_failed(detail);
                    Task::none()
                }
            }
        }
        RuleFormMessage::OpenTagInput => {
            if let Some(form) = view.form_mut() {
                form.update(RuleFormMessage::OpenTagInput);
            }
            operation::focus(crate::ui::rules::TAG_FIELD_ID)
        }
        field_change => {
            if let Some(form) = view.form_mut() {
                form.update(field_change);
            }
            schedule_rule_try(hermes)
        }
    }
}

pub fn open(hermes: &mut Hermes, ctx: &Ctx<'_>, actions: &mut Vec<Action>) -> Task<Message> {
    if hermes.rules.as_ref().is_some_and(|page| !page.is_form_only() || page.form().is_some()) {
        return Task::none();
    }
    actions.push(Action::CloseContextMenu);
    hermes.rules_generation = hermes.rules_generation.wrapping_add(1);
    hermes.rules = Some(RulesPage::new().with_kinds(Arc::clone(&ctx.kinds.catalog)));
    Task::batch([run_rules_load(hermes), super::targets::load_states(hermes, ctx)])
}

fn rules_access(hermes: &mut Hermes) -> Option<RulesAccess> {
    let access = hermes.filing.as_ref().and_then(FilingModule::rules_access);
    if access.is_none() {
        if let Some(view) = hermes.rules.as_mut() {
            view.failed(INDEX_NOT_OPEN.to_owned());
        }
    }
    access
}

/// Reads the rules again, if the page is up: what the targets page shows of
/// them changed behind the rules' own back.
pub(super) fn reload(hermes: &mut Hermes) -> Task<Message> {
    if hermes.rules.is_none() {
        return Task::none();
    }
    run_rules_load(hermes)
}

fn run_rules_load(hermes: &mut Hermes) -> Task<Message> {
    let generation = hermes.rules_generation;
    let Some(access) = rules_access(hermes) else {
        return Task::none();
    };
    Task::perform(
        tokio::task::spawn_blocking(move || access.load()),
        move |joined| {
            let result = joined.unwrap_or_else(|join| Err(join.to_string()));
            Message::Rules(RulesMessage::Loaded { generation, result })
        },
    )
}

fn run_rule_save(hermes: &mut Hermes, requested: Option<RuleSave>) -> Task<Message> {
    let Some(save) = requested else {
        return Task::none();
    };
    let generation = hermes.rules_generation;
    let Some(access) = rules_access(hermes) else {
        if let Some(form) = hermes.rules.as_mut().and_then(RulesPage::form_mut) {
            form.saving = false;
        }
        return Task::none();
    };
    Task::perform(
        tokio::task::spawn_blocking(move || access.save(save)),
        move |joined| {
            let result = joined.unwrap_or_else(|join| Err(join.to_string()));
            Message::Rules(RulesMessage::Form(RuleFormMessage::Saved {
                generation,
                result,
            }))
        },
    )
}

fn run_rule_change(hermes: &mut Hermes, requested: Option<RuleChange>) -> Task<Message> {
    let Some(change) = requested else {
        return Task::none();
    };
    let generation = hermes.rules_generation;
    let Some(access) = rules_access(hermes) else {
        return Task::none();
    };
    Task::perform(
        tokio::task::spawn_blocking(move || access.apply(change)),
        move |joined| {
            let result = joined.unwrap_or_else(|join| Err(join.to_string()));
            Message::Rules(RulesMessage::Changed { generation, result })
        },
    )
}
