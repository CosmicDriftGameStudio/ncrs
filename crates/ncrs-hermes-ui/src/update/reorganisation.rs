use iced::Task;
use ncrs_core::INDEX_NOT_OPEN;
use ncrs_filing::filing::FilingModule;
use ncrs_ui_kit::focus::release_focus;

use super::blocking_overlay_is_open;
use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::{HermesPage, Message, ReorganisationPlanMessage};
use crate::reorganisation_plan::ReorganisationView;
use crate::state::Hermes;

pub fn update(
    hermes: &mut Hermes,
    message: ReorganisationPlanMessage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match message {
        ReorganisationPlanMessage::Open => open(hermes, ctx, actions),
        ReorganisationPlanMessage::Close => {
            if hermes.reorganisation.take().is_some() {
                release_focus()
            } else {
                Task::none()
            }
        }
        ReorganisationPlanMessage::Replan => {
            let Some(view) = hermes.reorganisation.as_mut() else {
                return Task::none();
            };
            if view.is_busy() {
                return Task::none();
            }
            view.begin_planning();
            run_planning(hermes, ctx)
        }
        ReorganisationPlanMessage::Loaded { generation, result } => {
            if generation != hermes.reorganisation_generation {
                return Task::none();
            }
            match (hermes.reorganisation.as_mut(), result) {
                (Some(view), Ok(rows)) => view.loaded(rows),
                (Some(view), Err(detail)) => view.failed(detail),
                (None, _) => {}
            }
            Task::none()
        }
        ReorganisationPlanMessage::Move(delta) => {
            if let Some(view) = hermes.reorganisation.as_mut() {
                view.move_cursor(delta);
            }
            Task::none()
        }
        ReorganisationPlanMessage::Scrolled(delta) => {
            if let Some(view) = hermes.reorganisation.as_mut() {
                view.scroll(delta);
            }
            Task::none()
        }
        ReorganisationPlanMessage::Select(line) => {
            if let Some(view) = hermes.reorganisation.as_mut() {
                view.select(line);
            }
            Task::none()
        }
        ReorganisationPlanMessage::Toggle(line) => {
            if let Some(view) = hermes.reorganisation.as_mut() {
                view.toggle(line);
            }
            Task::none()
        }
        ReorganisationPlanMessage::ToggleCursor => {
            if let Some(view) = hermes.reorganisation.as_mut() {
                view.toggle_cursor();
            }
            Task::none()
        }
        ReorganisationPlanMessage::OpenRule(id) => {
            let Some(confirmed) = hermes
                .reorganisation
                .as_ref()
                .and_then(|view| {
                    view.rows()
                        .iter()
                        .filter_map(|planned| planned.rule.as_ref())
                        .find(|rule| rule.id == id)
                })
                .map(|rule| rule.confirmed)
            else {
                return Task::none();
            };
            let page = if confirmed {
                HermesPage::Rules
            } else {
                HermesPage::RuleSuggestions
            };
            actions.push(Action::OpenRuleOfPlan { id, page });
            Task::none()
        }
        ReorganisationPlanMessage::Execute => match hermes.reorganisation.as_ref() {
            Some(view) if view.has_selection() && !view.is_busy() => {
                Task::done(Message::ExecuteReorganisation(view.selection()))
            }
            _ => Task::none(),
        },
    }
}

/// The plan is made as the view opens; the entries appear when it is done.
pub fn open(hermes: &mut Hermes, ctx: &Ctx<'_>, actions: &mut Vec<Action>) -> Task<Message> {
    if hermes.reorganisation.is_some() || blocking_overlay_is_open(hermes, ctx) {
        return Task::none();
    }
    actions.push(Action::CloseContextMenu);
    hermes.reorganisation_generation = hermes.reorganisation_generation.wrapping_add(1);
    hermes.reorganisation = Some(ReorganisationView::new());
    run_planning(hermes, ctx)
}

/// The files moved, so an open plan is out of date.
pub fn replan_if_open(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Task<Message> {
    let Some(view) = hermes.reorganisation.as_mut() else {
        return Task::none();
    };
    view.begin_planning();
    hermes.reorganisation_generation = hermes.reorganisation_generation.wrapping_add(1);
    run_planning(hermes, ctx)
}

/// Plans on a blocking thread: the plan reads the whole folder tree and holds
/// the index the intake shares.
fn run_planning(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Task<Message> {
    let generation = hermes.reorganisation_generation;
    let Some(access) = ctx.filing.and_then(FilingModule::plan_access) else {
        if let Some(view) = hermes.reorganisation.as_mut() {
            view.failed(INDEX_NOT_OPEN.to_owned());
        }
        return Task::none();
    };
    Task::perform(
        tokio::task::spawn_blocking(move || access.plan()),
        move |joined| {
            let result = joined.unwrap_or_else(|join| Err(join.to_string()));
            Message::ReorganisationPlan(ReorganisationPlanMessage::Loaded { generation, result })
        },
    )
}
