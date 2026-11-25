//! The dialogs in which Hermes proposes rules: the work runs in the
//! background, nothing is saved before the user takes it.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use iced::Task;
use ncrs_core::INDEX_NOT_OPEN;
use ncrs_filing::filing::Phase;
use ncrs_filing::sorting::{self, SuggestError, Suggested};
use ncrs_index::{Rule, RuleId, RuleOrigin, TargetId};
use ncrs_intake::SortSource;

use super::Outcome;
use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::{Message, RulesMessage};
use crate::rule_form::{RuleForm, RuleFormTarget};
use crate::rules_page::RulesPage;
use crate::state::Hermes;
use crate::suggest::{Editing, Found, Suggest, SuggestFor, SuggestMessage};

fn send(message: SuggestMessage) -> Message {
    Message::Suggest(message)
}

/// "Schema vorschlagen" and "Regeln vorschlagen" on a target; the pattern
/// `wish` comes first.
pub fn open_scheme(
    hermes: &mut Hermes,
    target: TargetId,
    wish: Option<String>,
    ctx: &Ctx<'_>,
) -> Outcome {
    open(
        hermes,
        Suggest {
            wish,
            ..Suggest::new(SuggestFor::Target(target), 0)
        },
        ctx,
    )
}

/// "Regel vorschlagen" on a file of a source that no rule takes.
pub fn open_rule(hermes: &mut Hermes, source: SortSource, path: PathBuf, ctx: &Ctx<'_>) -> Outcome {
    open(
        hermes,
        Suggest::new(SuggestFor::File { source, path }, 0),
        ctx,
    )
}

fn open(hermes: &mut Hermes, dialog: Suggest, ctx: &Ctx<'_>) -> Outcome {
    if let Some(old) = hermes.suggest.take() {
        old.stop();
    }
    hermes.suggest = Some(dialog);
    (start(hermes, ctx), Vec::new())
}

/// The index may be open now, which a dialog over a target just added waits
/// for.
pub fn resume(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Task<Message> {
    let waits = hermes
        .suggest
        .as_ref()
        .is_some_and(|state| state.waits_for_index);
    if waits {
        start(hermes, ctx)
    } else {
        Task::none()
    }
}

/// The first target makes the module start anew.
fn index_is_opening(hermes: &Hermes) -> bool {
    hermes.filing_restart.is_some()
        || hermes
            .filing
            .as_ref()
            .is_some_and(|module| matches!(module.phase(), Phase::Starting))
}

/// The model of the setting, when Hermes is on and one can be had; the
/// proposals come from the folders and files alone without it.
fn model_of(
    hermes: &Hermes,
) -> impl FnOnce() -> Option<Arc<dyn ncrs_ai::ChatModel>> + Send + 'static {
    let setting = hermes.filing_config.ai.clone();
    let load = hermes.filing_config.load;
    let index_path = hermes.index_path.clone();
    let on = hermes.filing_config.hermes_on();
    move || {
        if !on {
            return None;
        }
        ncrs_filing::filing::chat_model(&setting, load, &index_path?).ok()
    }
}

fn start(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Task<Message> {
    let Some(state) = hermes.suggest.as_ref() else {
        return Task::none();
    };
    let (subject, again, generation) = (state.subject.clone(), state.again, state.generation);
    let wish = state.wish.clone();
    let stop: Arc<AtomicBool> = Arc::clone(&state.stop);
    let access = hermes
        .filing
        .as_ref()
        .and_then(|module| module.sorting_access(Arc::clone(&ctx.trash.0)));
    let opening = access.is_none() && index_is_opening(hermes);
    if let Some(state) = hermes.suggest.as_mut() {
        state.waits_for_index = opening;
        if access.is_none() && !opening {
            state.failed(SuggestError::Index(INDEX_NOT_OPEN.to_owned()));
        }
    }
    let Some(access) = access else {
        return Task::none();
    };
    let model_of = model_of(hermes);
    Task::perform(
        tokio::task::spawn_blocking(move || {
            let model = model_of();
            let asked = model.as_deref();
            let found = match subject {
                SuggestFor::Target(target) => sorting::suggest_scheme(
                    &access,
                    asked,
                    &target,
                    wish.as_deref(),
                    again,
                    Some(&stop),
                )
                .map(Found::Target),
                SuggestFor::File { source, path } => {
                    sorting::suggest_rule_for(&access, asked, &source, &path, Some(&stop))
                        .map(|found| Found::File(Box::new(found)))
                }
            };
            found.map(Box::new)
        }),
        move |joined| {
            send(SuggestMessage::Loaded {
                generation,
                result: joined.unwrap_or_else(|join| Err(SuggestError::Index(join.to_string()))),
            })
        },
    )
}

pub fn update(hermes: &mut Hermes, message: SuggestMessage, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = handle(hermes, message, ctx, &mut actions);
    (task, actions)
}

fn handle(
    hermes: &mut Hermes,
    message: SuggestMessage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match message {
        SuggestMessage::Loaded { generation, result } => {
            let Some(state) = hermes
                .suggest
                .as_mut()
                .filter(|s| s.generation == generation)
            else {
                return Task::none();
            };
            match result {
                Ok(found) => state.loaded(*found),
                Err(error) => state.failed(error),
            }
            Task::none()
        }
        SuggestMessage::Toggle(at) => {
            if let Some(state) = hermes.suggest.as_mut() {
                state.toggle(at);
            }
            Task::none()
        }
        SuggestMessage::CursorUp => move_cursor(hermes, -1),
        SuggestMessage::CursorDown => move_cursor(hermes, 1),
        SuggestMessage::ToggleCursor => {
            if let Some(state) = hermes.suggest.as_mut() {
                state.toggle(state.cursor);
            }
            Task::none()
        }
        SuggestMessage::Again => {
            if let Some(state) = hermes.suggest.as_mut() {
                state.restart();
            }
            start(hermes, ctx)
        }
        SuggestMessage::Edit(at) => edit(hermes, at),
        SuggestMessage::Reject(at) => reject(hermes, at, ctx),
        SuggestMessage::Rejected { generation, result } => {
            if let (Some(state), Err(detail)) = (
                hermes
                    .suggest
                    .as_mut()
                    .filter(|s| s.generation == generation),
                result,
            ) {
                state.problem = Some(detail);
            }
            Task::none()
        }
        SuggestMessage::Adopt => adopt(hermes, ctx),
        SuggestMessage::Adopted { generation, result } => {
            let Some(state) = hermes
                .suggest
                .as_mut()
                .filter(|s| s.generation == generation)
            else {
                return Task::none();
            };
            match result {
                Ok(()) => {
                    hermes.suggest = None;
                    hermes.watching.mark_all();
                    actions.push(Action::RulesChanged);
                    reload_rules(hermes)
                }
                Err(detail) => {
                    state.saving = false;
                    state.problem = Some(detail);
                    Task::none()
                }
            }
        }
        SuggestMessage::Close => {
            close(hermes);
            Task::none()
        }
    }
}

fn move_cursor(hermes: &mut Hermes, delta: isize) -> Task<Message> {
    if let Some(state) = hermes.suggest.as_mut() {
        state.move_cursor(delta);
    }
    Task::none()
}

/// Ends the work and takes the dialog down; what was ticked is dropped.
pub fn close(hermes: &mut Hermes) {
    if let Some(state) = hermes.suggest.take() {
        state.stop();
    }
}

/// The proposals the dialog would save: the ticked ones, or the rule of a
/// file.
fn taken(state: &Suggest) -> Vec<Suggested> {
    match state.file_rule() {
        Some(found) => vec![found.rule.clone()],
        None => state.ticked(),
    }
}

/// "Ändern": the proposal opens in the rule form of the rules page over the
/// dialog, unsaved. Saving the form makes the rule and takes the proposal
/// out of the dialog; cancelling leaves the dialog as it was.
fn edit(hermes: &mut Hermes, at: usize) -> Task<Message> {
    let Some(state) = hermes.suggest.as_ref() else {
        return Task::none();
    };
    let (Some(target), Some(suggested)) = (
        state.target(),
        state
            .file_rule()
            .map(|found| found.rule.clone())
            .or_else(|| state.items().get(at).cloned()),
    ) else {
        return Task::none();
    };
    let rule = Rule {
        id: RuleId::unused_in(&[]),
        position: 1,
        origin: RuleOrigin::Suggestion,
        target,
        spec: suggested.spec,
    };
    let mut form = RuleForm::from_suggestion(&rule);
    form.target = RuleFormTarget::New;
    let fresh = hermes.rules.is_none();
    hermes
        .rules
        .get_or_insert_with(RulesPage::new)
        .open_form(form);
    if let Some(state) = hermes.suggest.as_mut() {
        state.editing = Some(Editing {
            at,
            rules_before: None,
        });
    }
    after_rules(hermes);
    if fresh {
        reload_rules(hermes)
    } else {
        Task::none()
    }
}

/// Run after every message and every close of the rules page: how the form
/// that "Ändern" opened ended. A rule more in the target means it was saved.
pub fn after_rules(hermes: &mut Hermes) {
    let Some(state) = hermes.suggest.as_mut() else {
        return;
    };
    let Some(target) = state.target() else {
        return;
    };
    let (Some(editing), Some(page)) = (state.editing.as_mut(), hermes.rules.as_ref()) else {
        return;
    };
    if page.is_busy() {
        return;
    }
    let count = page.rows_of(&target).count();
    if page.form().is_some() {
        editing.rules_before.get_or_insert(count);
        return;
    }
    let saved = editing.rules_before.is_some_and(|before| count > before);
    let at = editing.at;
    state.editing = None;
    if !saved {
        return;
    }
    if state.file_rule().is_some() {
        hermes.suggest = None;
    } else {
        state.remove(at);
    }
}

/// "Verwerfen": the user turns the proposal down; it does not come back.
fn reject(hermes: &mut Hermes, at: usize, ctx: &Ctx<'_>) -> Task<Message> {
    let Some(state) = hermes.suggest.as_mut() else {
        return Task::none();
    };
    let (Some(target), Some(suggested)) = (state.target(), state.items().get(at).cloned()) else {
        return Task::none();
    };
    let generation = state.generation;
    state.remove(at);
    let Some(access) = hermes
        .filing
        .as_ref()
        .and_then(|module| module.sorting_access(Arc::clone(&ctx.trash.0)))
    else {
        return Task::none();
    };
    Task::perform(
        tokio::task::spawn_blocking(move || {
            sorting::dismiss(&access, &target, &[suggested]).map_err(|error| error.to_string())
        }),
        move |joined| {
            send(SuggestMessage::Rejected {
                generation,
                result: joined.unwrap_or_else(|join| Err(join.to_string())),
            })
        },
    )
}

fn adopt(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Task<Message> {
    let Some(state) = hermes.suggest.as_mut().filter(|state| !state.saving) else {
        return Task::none();
    };
    let (Some(target), true) = (state.target(), state.found.is_some()) else {
        return Task::none();
    };
    let take = taken(state);
    if take.is_empty() {
        return Task::none();
    }
    let generation = state.generation;
    let Some(access) = hermes
        .filing
        .as_ref()
        .and_then(|module| module.sorting_access(Arc::clone(&ctx.trash.0)))
    else {
        return Task::none();
    };
    state.saving = true;
    state.problem = None;
    Task::perform(
        tokio::task::spawn_blocking(move || sorting::adopt(&access, &target, &take).map(|_| ())),
        move |joined| {
            send(SuggestMessage::Adopted {
                generation,
                result: joined
                    .map_err(|join| join.to_string())
                    .and_then(|done| done.map_err(|error| error.to_string())),
            })
        },
    )
}

fn reload_rules(hermes: &Hermes) -> Task<Message> {
    if hermes.rules.is_none() {
        return Task::none();
    }
    Task::done(Message::Rules(RulesMessage::Changed {
        generation: hermes.rules_generation,
        result: Ok(()),
    }))
}
