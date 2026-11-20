//! When the sync runs, and what the tab does with its answer. The copying
//! itself is `sync::run`, on a blocking thread.

use std::time::{Duration, Instant, SystemTime};

use iced::Task;
use ncrs_core::notes_sync::{NotesSyncDirection, NotesSyncWhen};
use ncrs_i18n::{fill_template, Msg};

use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::State;
use crate::sync::{self, SyncJob, SyncReport};
use crate::update::reload;

/// Starts a run unless one is going; a change that comes meanwhile makes the
/// next run follow it.
pub fn start(state: &mut State, ctx: &Ctx, now: Instant) -> Task<Message> {
    let Some(target) = state.options.sync_folder.clone() else {
        return Task::none();
    };
    if state.sync.running {
        state.sync.again = true;
        return Task::none();
    }
    state.sync.running = true;
    state.sync.again = false;
    state.sync.started_at = Some(now);
    let job = SyncJob {
        local: state.root.clone(),
        target,
        direction: state.options.direction,
        conflict_suffix: ctx.lang.text(Msg::NotesConflictSuffix).to_owned(),
    };
    Task::perform(
        async move { tokio::task::spawn_blocking(move || sync::run(&job, SystemTime::now())).await },
        |joined| Message::SyncDone(joined.unwrap_or_else(|err| Err(err.to_string()))),
    )
}

/// A note was written: "after saving" syncs once the writes are done.
pub fn after_save(state: &mut State, ctx: &Ctx) -> Task<Message> {
    if state.options.when != NotesSyncWhen::AfterSave
        || state.options.sync_folder.is_none()
        || state.busy()
    {
        return Task::none();
    }
    start(state, ctx, Instant::now())
}

/// The clock: "at start" runs once when the notes are read, "every N minutes"
/// when the interval has passed.
pub fn on_tick(state: &mut State, now: Instant, ctx: &Ctx) -> Task<Message> {
    if state.options.sync_folder.is_none() || state.sync.running {
        return Task::none();
    }
    let due = match state.options.when {
        NotesSyncWhen::AfterSave => false,
        NotesSyncWhen::AtStart => state.loaded && state.sync.started_at.is_none(),
        NotesSyncWhen::EveryMinutes(minutes) => {
            state.loaded
                && state.sync.started_at.is_none_or(|last| {
                    now.saturating_duration_since(last)
                        >= Duration::from_secs(u64::from(minutes) * 60)
                })
        }
    };
    if due {
        start(state, ctx, now)
    } else {
        Task::none()
    }
}

pub fn handle(state: &mut State, message: Message, ctx: &Ctx) -> (Task<Message>, Option<Action>) {
    match message {
        Message::SyncNow => {
            if state.options.sync_folder.is_none() {
                let text = ctx.lang.text(Msg::NotesSyncNoFolder).to_owned();
                return (Task::none(), Some(Action::Error(text)));
            }
            (start(state, ctx, Instant::now()), None)
        }
        Message::SyncDone(result) => done(state, result, ctx),
        _ => (Task::none(), None),
    }
}

fn done(
    state: &mut State,
    result: Result<SyncReport, String>,
    ctx: &Ctx,
) -> (Task<Message>, Option<Action>) {
    state.sync.running = false;
    match result {
        Ok(report) => {
            let arrived =
                report.copied > 0 && state.options.direction == NotesSyncDirection::BothWays;
            state.sync.last = Some(report);
            state.sync.error = None;
            let mut tasks = Vec::new();
            if arrived {
                tasks.push(reload(state));
            }
            if state.sync.again {
                tasks.push(start(state, ctx, Instant::now()));
            }
            (Task::batch(tasks), None)
        }
        Err(reason) => {
            state.sync.again = false;
            let text = fill_template(
                ctx.lang.text(Msg::NotesSyncFailed),
                &[("{reason}", &reason)],
            );
            state.sync.error = Some(reason);
            (Task::none(), Some(Action::Error(text)))
        }
    }
}
