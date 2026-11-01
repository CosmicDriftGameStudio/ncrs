use iced::Task;
use ncrs_core::INDEX_NOT_OPEN;
use ncrs_filing::execute::UndoReport;
use ncrs_filing::filing::FilingModule;
use ncrs_filing::reorganisation_work::{ExecutionSummary, PlannedItem};
use ncrs_i18n::Msg;
use ncrs_jobs::{Job, JobKind};
use ncrs_ui_kit::focus::release_focus;

use super::{blocking_overlay_is_open, open_rule_of_plan};
use crate::action::{Action, StartProgressJob};
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
            run_planning(hermes)
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
            open_rule_of_plan(hermes, id, page, ctx, actions)
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
    run_planning(hermes)
}

/// The files moved, so an open plan is out of date.
pub fn replan_if_open(hermes: &mut Hermes) -> Task<Message> {
    let Some(view) = hermes.reorganisation.as_mut() else {
        return Task::none();
    };
    view.begin_planning();
    hermes.reorganisation_generation = hermes.reorganisation_generation.wrapping_add(1);
    run_planning(hermes)
}

/// Plans on a blocking thread: the plan reads the whole folder tree and holds
/// the index the intake shares.
fn run_planning(hermes: &mut Hermes) -> Task<Message> {
    let generation = hermes.reorganisation_generation;
    let Some(access) = hermes.filing.as_ref().and_then(FilingModule::plan_access) else {
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

/// Runs the selected plan entries on the blocking pool, as one job. Escape
/// closes the progress channel, which stops the run between two files.
pub fn run_selection(
    hermes: &Hermes,
    selection: Vec<PlannedItem>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) {
    if ctx.jobs_busy || selection.is_empty() {
        return;
    }
    let Some(access) = hermes.filing.as_ref().and_then(FilingModule::plan_access) else {
        actions.push(Action::ShowError(INDEX_NOT_OPEN.to_owned()));
        return;
    };
    let total = selection.len();
    let path = selection
        .first()
        .map(|entry| entry.location.clone())
        .unwrap_or_default();
    actions.push(Action::SetJobProgress(ncrs_fs::transfer::Tick {
        done: 0,
        total,
    }));
    actions.push(Action::StartJobWithProgress(StartProgressJob {
        job: Job {
            kind: JobKind::Reorganise,
            path,
            total: Some(total),
        },
        build: Box::new(move |progress_tx| {
            Task::perform(
                tokio::task::spawn_blocking(move || {
                    let mut progress = ncrs_fs::transfer::Reporting::new(&progress_tx);
                    access.execute(&selection, &mut progress)
                }),
                |joined| {
                    Message::ReorganisationExecuted(
                        joined.unwrap_or_else(|join| Err(join.to_string())),
                    )
                },
            )
        }),
    }));
}

/// The run of the plan is over.
pub fn executed(
    hermes: &mut Hermes,
    result: Result<ExecutionSummary, String>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    actions.extend([Action::FinishJob, Action::ClearJobProgress]);
    match result {
        Ok(summary) => {
            let text = ctx
                .lang
                .text(Msg::StatusReorganised)
                .replace("{moved}", &summary.moved.to_string())
                .replace("{skipped}", &summary.skipped.to_string());
            actions.push(Action::ShowNotice(text));
        }
        Err(detail) => actions.push(Action::ShowError(detail)),
    }
    replan_if_open(hermes)
}

pub fn undo(hermes: &Hermes, ctx: &Ctx<'_>, actions: &mut Vec<Action>) -> Task<Message> {
    if ctx.jobs_busy {
        return Task::none();
    }
    let Some(access) = hermes.filing.as_ref().and_then(FilingModule::plan_access) else {
        actions.push(Action::ShowError(INDEX_NOT_OPEN.to_owned()));
        return Task::none();
    };
    Task::perform(
        tokio::task::spawn_blocking(move || access.undo_last()),
        |joined| Message::ReorganisationUndone(joined.unwrap_or_else(|join| Err(join.to_string()))),
    )
}

pub fn undone(
    hermes: &mut Hermes,
    result: Result<Option<UndoReport>, String>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match result {
        Ok(Some(report)) => {
            let text = ctx
                .lang
                .text(Msg::StatusReorganisationUndone)
                .replace("{moved}", &report.undone.to_string())
                .replace("{skipped}", &report.left.to_string());
            actions.push(Action::ShowNotice(text));
            replan_if_open(hermes)
        }
        Ok(None) => {
            let text = ctx.lang.text(Msg::StatusNothingToUndo).to_owned();
            actions.push(Action::ShowNotice(text));
            Task::none()
        }
        Err(detail) => {
            actions.push(Action::ShowError(detail));
            Task::none()
        }
    }
}
