//! Starting, replacing and stopping the filing module and its pipeline, and
//! the steps of the cards that lead there.

use std::sync::Arc;

use iced::widget::operation;
use iced::Task;
use ncrs_config::config::Filing;
use ncrs_filing::filing::{FilingMessage, FilingModule, LocationStatus, Phase, Work};
use ncrs_i18n::{fill_template, Msg};

use super::filing_job::{run_filing, run_filing_work, run_filing_work_after};
use crate::action::{Action, ProtectionStep};
use crate::ctx::Ctx;
use crate::message::{FilingStatusMessage, Message, TagsMessage};
use crate::state::Hermes;
use crate::ui::filing as filing_ui;

/// When a saved filing list takes effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavedFilingApplies {
    Now,
    AtNextStart,
}

/// Replaces nothing: the caller has dropped any module and pipeline before.
pub fn start_filing_module(hermes: &mut Hermes, filing: &Filing, ctx: &Ctx<'_>) -> Task<Message> {
    hermes.filing_config = filing.clone();
    // A new runner counts its plans from zero again; the old count would
    // keep its first plans from ever being filed.
    hermes.plans_filed_for = 0;
    let Some(store) = hermes.key_store.clone() else {
        return Task::none();
    };
    let Some((mut module, work)) = FilingModule::start(filing, hermes.index_path.clone(), store.0)
    else {
        return Task::none();
    };
    module.set_kind_language(ctx.lang.kind_language());
    module.set_kinds(Arc::clone(&ctx.kinds.catalog));
    hermes.filing = Some(module);
    work.map_or_else(Task::none, run_filing_work)
}

/// Dropping the runner stops it and lets go of the index before the new
/// module opens it again.
fn replace_module(hermes: &mut Hermes, filing: &Filing, ctx: &Ctx<'_>) -> Task<Message> {
    hermes.pipeline = None;
    hermes.filing = None;
    let task = start_filing_module(hermes, filing, ctx);
    hermes.refresh_filing_snapshot(ctx.now);
    task
}

pub fn saved_filing_notice(applies: SavedFilingApplies, ctx: &Ctx<'_>) -> String {
    match applies {
        SavedFilingApplies::AtNextStart => ctx.lang.text(Msg::FilingSetupSavedNextStart).to_owned(),
        SavedFilingApplies::Now => match ctx.filing_status_shortcut.as_deref() {
            Some(key) => fill_template(
                ctx.lang.text(Msg::FilingSetupSavedRunning),
                &[("{key}", key)],
            ),
            None => ctx
                .lang
                .text(Msg::FilingSetupSavedRunningUnbound)
                .to_owned(),
        },
    }
}

/// A module that works on configured locations keeps running on them: there
/// is no reload, and the saved list takes over at the next start. One
/// without locations, or none at all, is replaced at once.
pub fn apply_saved_filing(
    hermes: &mut Hermes,
    filing: Filing,
    ctx: &Ctx<'_>,
) -> (SavedFilingApplies, Task<Message>) {
    let replaceable = hermes.key_store.is_some()
        && hermes
            .filing
            .as_ref()
            .is_none_or(|module| !module.has_configured_locations() && module.can_be_replaced());
    if !replaceable {
        hermes.filing_config = filing;
        return (SavedFilingApplies::AtNextStart, Task::none());
    }
    (
        SavedFilingApplies::Now,
        replace_module(hermes, &filing, ctx),
    )
}

/// An AI setting changed. The module is rebuilt now when nothing is moving
/// files and it may be replaced; otherwise the saved config takes over at the
/// next start. Joining the runner's threads blocks, so both are dropped on
/// the blocking pool.
pub fn apply_ai_change(
    hermes: &mut Hermes,
    filing: Filing,
    ctx: &Ctx<'_>,
) -> (SavedFilingApplies, Task<Message>) {
    if let Some((_, pending)) = hermes.filing_restart.as_mut() {
        hermes.filing_config = filing.clone();
        *pending = filing;
        return (SavedFilingApplies::Now, Task::none());
    }
    let Some(module) = hermes.filing.as_ref() else {
        return apply_saved_filing(hermes, filing, ctx);
    };
    let rebuildable = hermes.key_store.is_some() && module.can_be_replaced() && !ctx.jobs_busy;
    if !rebuildable {
        let switched_off = !filing.hermes_on();
        hermes.filing_config = filing;
        if switched_off {
            if let Some(runner) = hermes.live_pipeline() {
                runner.pause();
            }
            hermes.refresh_filing_snapshot(ctx.now);
        }
        return (SavedFilingApplies::AtNextStart, Task::none());
    }
    hermes.filing_restart_generation = hermes.filing_restart_generation.wrapping_add(1);
    let generation = hermes.filing_restart_generation;
    hermes.filing_config = filing.clone();
    hermes.filing_restart = Some((generation, filing));
    drop(hermes.reading_again.give_up());
    let pipeline = hermes.pipeline.take();
    let old_module = hermes.filing.take();
    hermes.refresh_filing_snapshot(ctx.now);
    let dropped = Task::perform(
        tokio::task::spawn_blocking(move || {
            drop(pipeline);
            drop(old_module);
        }),
        move |_| Message::FilingStatus(FilingStatusMessage::Restarted(generation)),
    );
    (SavedFilingApplies::Now, dropped)
}

/// The old module and runner are gone from the blocking pool: the new one
/// starts from the config the change left.
pub fn filing_restarted(hermes: &mut Hermes, generation: u64, ctx: &Ctx<'_>) -> Task<Message> {
    match hermes.filing_restart.take() {
        Some((current, filing)) if current == generation => {
            let task = start_filing_module(hermes, &filing, ctx);
            hermes.refresh_filing_snapshot(ctx.now);
            task
        }
        other => {
            hermes.filing_restart = other;
            Task::none()
        }
    }
}

/// Builds the module again from the config it started with, when it is not in
/// the middle of something.
pub fn restart_hermes_module(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Task<Message> {
    let can_restart = hermes.key_store.is_some()
        && hermes
            .filing
            .as_ref()
            .is_some_and(FilingModule::can_be_replaced);
    if !can_restart {
        return Task::none();
    }
    let config = hermes.filing_config.clone();
    replace_module(hermes, &config, ctx)
}

/// Stops the pipeline and runs the job that encrypts or removes the index.
/// Nothing happens while files move or the module is being rebuilt.
pub fn change_protection(
    hermes: &mut Hermes,
    begin: ProtectionStep,
    ctx: &Ctx<'_>,
) -> Task<Message> {
    if ctx.jobs_busy || hermes.is_restarting() {
        return Task::none();
    }
    let Some(work) = hermes.filing.as_mut().and_then(begin) else {
        return Task::none();
    };
    drop(hermes.reading_again.give_up());
    hermes.plans_filed_for = 0;
    let pipeline = hermes.pipeline.take();
    hermes.refresh_filing_snapshot(ctx.now);
    run_filing_work_after(pipeline, work)
}

/// A module without an index creates it by itself, always encrypted. A first
/// failure ends in the module's error card, so this does not repeat.
fn first_run_creation(module: &mut FilingModule) -> Option<Work> {
    let waits = module.asks_encryption()
        && !module.is_creating_index()
        && module.index_creation_error().is_none();
    if waits {
        module.create_index_with(true)
    } else {
        None
    }
}

pub fn update_filing(
    hermes: &mut Hermes,
    message: FilingMessage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    let Some(module) = hermes.filing.as_mut() else {
        return Task::none();
    };
    if matches!(message, FilingMessage::CopyRecoveryKey) {
        let Some(key_text) = module.recovery_key_text() else {
            return Task::none();
        };
        actions.push(Action::ShowNotice(
            ctx.lang.text(Msg::FilingKeyCopied).to_owned(),
        ));
        return iced::clipboard::write(key_text);
    }
    let work_finished = matches!(message, FilingMessage::WorkDone { .. });
    let was_starting = matches!(module.phase(), Phase::Starting);
    let was_starting_over = matches!(module.phase(), Phase::StartingOver);
    let work = module.update(message);
    if was_starting_over && !matches!(module.phase(), Phase::StartingOver) {
        actions.push(Action::OpenChat);
    }
    let mut pipeline_started = false;
    if hermes.pipeline.is_none() && matches!(module.phase(), Phase::Ready) {
        let started = module.start_pipeline().ok();
        pipeline_started = started.is_some();
        (hermes.pipeline, hermes.pdf_library_missing) = match started {
            Some((runner, missing)) => (Some(runner), missing),
            None => (None, None),
        };
    }
    let skipped = if was_starting && !matches!(module.phase(), Phase::Starting) {
        module
            .locations()
            .iter()
            .filter(|entry| !matches!(entry.status, LocationStatus::Ready(_)))
            .count()
    } else {
        0
    };
    // A text field only gets the keys once it has the focus.
    let field = match module.phase() {
        Phase::ShowRecoveryKey { .. } if work_finished => Some(filing_ui::GROUP_FIELD_ID),
        Phase::AskRecoveryKey { .. } if work_finished => Some(filing_ui::KEY_FIELD_ID),
        _ => None,
    };
    let creation = first_run_creation(module);
    let exceptions_access = work_finished.then(|| module.tags_access()).flatten();
    let mut tasks: Vec<Task<Message>> = work
        .into_iter()
        .chain(creation)
        .map(run_filing_work)
        .collect();
    tasks.extend(exceptions_access.map(|access| {
        Task::perform(
            tokio::task::spawn_blocking(move || access.exceptions()),
            |joined| {
                let result = joined.unwrap_or_else(|join| Err(join.to_string()));
                Message::Tags(TagsMessage::ExceptionsRead(result))
            },
        )
    }));
    tasks.extend(field.map(operation::focus));
    if pipeline_started {
        // The index is open and `reconcile` has settled what a crash left;
        // what an earlier run planned but did not move goes now.
        run_filing(hermes, ctx, actions);
    }
    if skipped > 0 {
        let text = fill_template(
            ctx.lang.text(Msg::FilingLocationsSkipped),
            &[("{count}", &skipped.to_string())],
        );
        actions.push(Action::ShowNotice(text));
    }
    Task::batch(tasks)
}
