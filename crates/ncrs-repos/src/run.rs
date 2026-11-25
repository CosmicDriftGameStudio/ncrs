//! Starting, feeding and ending a bulk action. At most `concurrency` repos run
//! at once: the rest wait in a queue and the next one starts when one ends.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use iced::Task;
use ncrs_i18n::Msg;

use crate::bulk::{BulkAction, Executed, Outcome};
use crate::ctx::Ctx;
use crate::log::{clock_text, Log, LogLine};
use crate::message::Message;
use crate::rows::name_of;
use crate::state::{Run, Scope, State};
use crate::summary::Reading;
use crate::tasks;
use crate::texts::log_text;

/// Starts `action` on `targets`. Nothing happens while a run is going, without
/// targets, or where the write actions are off.
pub fn start(
    state: &mut State,
    action: BulkAction,
    (scope, mut targets): (Scope, Vec<PathBuf>),
    ctx: &Ctx,
) -> Task<Message> {
    if !ctx.actions_available || state.run.as_ref().is_some_and(|run| !run.is_finished()) {
        return Task::none();
    }
    if targets.is_empty() {
        return Task::none();
    }
    targets.sort_by_cached_key(|path| (name_of(path).to_lowercase(), path.clone()));
    state.run = Some(Run {
        action,
        scope,
        queue: VecDeque::from(targets.clone()),
        order: targets,
        results: BTreeMap::new(),
        running: 0,
        only_problems: false,
        details: None,
        log: Log::default(),
    });
    pump(state, ctx)
}

/// Starts as many waiting repos as there are free places.
pub fn pump(state: &mut State, ctx: &Ctx) -> Task<Message> {
    let limit = usize::from(ctx.settings.concurrency).max(1);
    let mut started = Vec::new();
    while let Some(run) = state.run.as_mut() {
        if run.running >= limit {
            break;
        }
        let Some(path) = run.queue.pop_front() else {
            break;
        };
        match state.readings.get(&path) {
            Some(Reading::Read(summary)) => {
                let is_worktree = summary
                    .main_worktree
                    .as_ref()
                    .is_some_and(|main| main != &path);
                run.running += 1;
                run.results.insert(path.clone(), Outcome::Running);
                started.push(tasks::step(
                    run.action,
                    path,
                    (**summary).clone(),
                    is_worktree,
                    Arc::clone(&state.stop),
                ));
            }
            other => {
                let last_line = match other {
                    Some(Reading::Gone) => ctx.lang.text(Msg::ReposFolderGone).to_owned(),
                    Some(Reading::Unreadable(why)) => why.clone(),
                    _ => String::new(),
                };
                let outcome = Outcome::Failed { last_line };
                log_line(&mut run.log, ctx, &path, run.action.command(), &outcome);
                run.results.insert(path, outcome);
            }
        }
    }
    Task::batch(started)
}

fn log_line(log: &mut Log, ctx: &Ctx, path: &Path, command: &str, outcome: &Outcome) {
    log.push(LogLine {
        time: clock_text(ctx.now),
        path: path.to_path_buf(),
        repo: name_of(path),
        command: command.to_owned(),
        result: log_text(outcome, ctx.lang),
        outcome: outcome.kind(),
    });
}

/// One repository ended: its result, its log lines and, when it was read
/// again, its new state. The caller pumps the queue.
pub fn finish(state: &mut State, ctx: &Ctx, path: PathBuf, executed: Executed) {
    let Some(run) = state.run.as_mut() else {
        return;
    };
    run.running = run.running.saturating_sub(1);
    if executed.ran.is_empty() {
        log_line(
            &mut run.log,
            ctx,
            &path,
            run.action.command(),
            &executed.outcome,
        );
    }
    for ran in &executed.ran {
        run.log.push(LogLine {
            time: clock_text(ctx.now),
            path: path.clone(),
            repo: name_of(&path),
            command: ran.command.clone(),
            result: ran.result.clone(),
            outcome: executed.outcome.kind(),
        });
    }
    run.results.insert(path.clone(), executed.outcome);
    if let Some(after) = executed.after {
        state.readings.insert(path, after);
    }
}

/// Runs the repositories again that failed.
pub fn retry_failed(state: &mut State, ctx: &Ctx) -> Task<Message> {
    if !ctx.actions_available {
        return Task::none();
    }
    let Some(run) = state.run.as_mut().filter(|run| run.is_finished()) else {
        return Task::none();
    };
    for path in run.failed() {
        run.results.remove(&path);
        run.queue.push_back(path);
    }
    pump(state, ctx)
}
