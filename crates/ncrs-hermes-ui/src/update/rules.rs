use std::sync::Arc;

use iced::widget::operation;
use iced::Task;
use ncrs_core::INDEX_NOT_OPEN;
use ncrs_filing::filing::FilingModule;
use ncrs_filing::rules_work::{RuleChange, RulesAccess};
use ncrs_ui_kit::focus::release_focus;

use super::{blocking_overlay_is_open, go_to_page, open_page};
use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::{HermesPage, Message, RuleFormMessage, RulesMessage};
use crate::rule_form::{RuleForm, RuleSave, TRY_DEBOUNCE};
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
        RulesMessage::Open => open(hermes, ctx, actions),
        RulesMessage::Close => close(hermes, ctx, actions),
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
        RulesMessage::Move(delta) => {
            if let Some(view) = hermes.rules.as_mut() {
                view.move_cursor(delta);
            }
            schedule_rule_try(hermes)
        }
        RulesMessage::Scrolled(delta) => {
            if let Some(view) = hermes.rules.as_mut() {
                view.scroll(delta);
            }
            schedule_rule_try(hermes)
        }
        RulesMessage::Select(index) => {
            if let Some(view) = hermes.rules.as_mut() {
                view.select(index);
            }
            Task::batch([release_focus(), schedule_rule_try(hermes)])
        }
        RulesMessage::ToggleTryList => {
            if let Some(view) = hermes.rules.as_mut() {
                view.toggle_try_list();
            }
            Task::none()
        }
        RulesMessage::Toggle => {
            let change = hermes
                .rules
                .as_mut()
                .and_then(|view| view.begin_toggle(view.cursor()));
            run_rule_change(hermes, change)
        }
        RulesMessage::ToggleAt(index) => {
            let change = hermes
                .rules
                .as_mut()
                .and_then(|view| view.begin_toggle(index));
            run_rule_change(hermes, change)
        }
        RulesMessage::Reorder(delta) => {
            let change = hermes
                .rules
                .as_mut()
                .and_then(|view| view.begin_move(delta));
            run_rule_change(hermes, change)
        }
        RulesMessage::AskDelete => {
            if let Some(view) = hermes.rules.as_mut() {
                view.ask_delete();
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
                Ok(()) => run_rules_load(hermes),
                Err(detail) => {
                    if let Some(view) = hermes.rules.as_mut() {
                        view.failed(detail);
                    }
                    Task::none()
                }
            }
        }
        RulesMessage::New => {
            if let Some(view) = hermes.rules.as_mut() {
                view.begin_new();
            }
            Task::batch([focus_rule_form(hermes), schedule_rule_try(hermes)])
        }
        RulesMessage::Edit => {
            if let Some(view) = hermes.rules.as_mut() {
                view.begin_edit();
            }
            Task::batch([focus_rule_form(hermes), schedule_rule_try(hermes)])
        }
        RulesMessage::Form(form_message) => update_rule_form(hermes, form_message, ctx, actions),
        RulesMessage::TryElapsed(generation) => run_rule_try(hermes, generation),
        RulesMessage::Tried { generation, result } => {
            if let Some(form) = hermes.rules.as_mut().and_then(RulesPage::form_mut) {
                form.try_answered(generation, result);
            }
            Task::none()
        }
    }
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

/// Closes the form without saving, if there is one, and goes back to the
/// page it was opened from.
fn cancel_rule_form(
    hermes: &mut Hermes,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Option<Task<Message>> {
    let view = hermes.rules.as_mut()?;
    if !view.cancel_form() {
        return None;
    }
    Some(match view.take_return_to() {
        Some(page) => go_to_page(hermes, page, ctx, actions),
        None => Task::batch([release_focus(), schedule_rule_try(hermes)]),
    })
}

/// Shows the Rules page with `form` open. From another Hermes page, or
/// from the panels, the page loads as the sidebar would load it.
pub fn open_rule_form(
    hermes: &mut Hermes,
    form: RuleForm,
    return_to: Option<HermesPage>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    let opened = if open_page(hermes, ctx).is_some() {
        go_to_page(hermes, HermesPage::Rules, ctx, actions)
    } else {
        open(hermes, ctx, actions)
    };
    let Some(view) = hermes.rules.as_mut() else {
        return opened;
    };
    view.open_form(form, return_to);
    Task::batch([opened, focus_rule_form(hermes), schedule_rule_try(hermes)])
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

fn run_rule_try(hermes: &mut Hermes, generation: u64) -> Task<Message> {
    let Some(request) = hermes
        .rules
        .as_mut()
        .and_then(RulesPage::form_mut)
        .and_then(|form| form.try_request(generation))
    else {
        return Task::none();
    };
    let Some(access) = hermes.filing.as_ref().and_then(FilingModule::rules_access) else {
        if let Some(form) = hermes.rules.as_mut().and_then(RulesPage::form_mut) {
            form.try_answered(generation, Err(INDEX_NOT_OPEN.to_owned()));
        }
        return Task::none();
    };
    Task::perform(
        tokio::task::spawn_blocking(move || access.try_rule(&request)),
        move |joined| {
            let result = joined.unwrap_or_else(|join| Err(join.to_string()));
            Message::Rules(RulesMessage::Tried { generation, result })
        },
    )
}

fn update_rule_form(
    hermes: &mut Hermes,
    message: RuleFormMessage,
    ctx: &Ctx<'_>,
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
            cancel_rule_form(hermes, ctx, actions).unwrap_or_else(Task::none)
        }
        RuleFormMessage::Saved { generation, result } => {
            if generation != hermes.rules_generation {
                return Task::none();
            }
            match result {
                Ok(id) => match view.saved(id) {
                    Some(page) => go_to_page(hermes, page, ctx, actions),
                    None => run_rules_load(hermes),
                },
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
            view.start_editing();
            operation::focus(crate::ui::rules::TAG_FIELD_ID)
        }
        field_change => {
            if let Some(form) = view.form_mut() {
                form.update(field_change);
            }
            view.start_editing();
            schedule_rule_try(hermes)
        }
    }
}

pub fn open(hermes: &mut Hermes, ctx: &Ctx<'_>, actions: &mut Vec<Action>) -> Task<Message> {
    if hermes.rules.is_some() || blocking_overlay_is_open(hermes, ctx) {
        return Task::none();
    }
    actions.push(Action::CloseContextMenu);
    hermes.rules_generation = hermes.rules_generation.wrapping_add(1);
    hermes.rules = Some(RulesPage::new().with_kinds(Arc::clone(&ctx.kinds.catalog)));
    run_rules_load(hermes)
}

/// Escape backs out of the delete question, the list of the try, a form
/// that is not listed yet and the fields, then leaves the page.
fn close(hermes: &mut Hermes, ctx: &Ctx<'_>, actions: &mut Vec<Action>) -> Task<Message> {
    let Some(view) = hermes.rules.as_mut() else {
        return Task::none();
    };
    if view.cancel_delete() {
        return Task::none();
    }
    if view.is_try_list_open() {
        view.toggle_try_list();
        return Task::none();
    }
    if view.has_unlisted_form() {
        return cancel_rule_form(hermes, ctx, actions).unwrap_or_else(Task::none);
    }
    if view.stop_editing() {
        return release_focus();
    }
    hermes.rules = None;
    release_focus()
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
