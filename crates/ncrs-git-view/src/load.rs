//! The blocking reads of the repository, started on the blocking pool. A
//! newer read of the same kind stops the older one through its flag.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use iced::Task;
use ncrs_git::{ActionError, DiffTarget, GitError};

use crate::job::{Finished, Job};
use crate::message::Message;
use crate::snapshot::{self, Want};
use crate::state::State;

fn blocking<T, Work>(
    stop: Arc<AtomicBool>,
    work: Work,
    done: impl Fn(Result<T, String>) -> Message + Send + 'static,
) -> Task<Message>
where
    T: Send + 'static,
    Work: FnOnce(&Arc<AtomicBool>) -> Result<T, GitError> + Send + 'static,
{
    Task::perform(
        async move { tokio::task::spawn_blocking(move || work(&stop)).await },
        move |joined| {
            done(match joined {
                Ok(Ok(value)) => Ok(value),
                Ok(Err(error)) => Err(error.to_string()),
                Err(error) => Err(error.to_string()),
            })
        },
    )
}

fn root_of(state: &State) -> Option<PathBuf> {
    state.root.clone()
}

pub fn refs(state: &mut State) -> Task<Message> {
    let Some(root) = root_of(state) else {
        return Task::none();
    };
    let (generation, stop) = state.loads.refs.begin();
    blocking(
        stop,
        move |cancelled| ncrs_git::refs(&root, cancelled),
        move |result| Message::RefsLoaded(generation, Box::new(result)),
    )
}

pub fn log(state: &mut State) -> Task<Message> {
    let Some(root) = root_of(state) else {
        return Task::none();
    };
    let (limit, filter) = (state.limit, state.filter.clone());
    let (generation, stop) = state.loads.log.begin();
    blocking(
        stop,
        move |cancelled| ncrs_git::log(&root, limit, &filter, cancelled),
        move |result| Message::LogLoaded(generation, Box::new(result)),
    )
}

pub fn commit_detail(state: &mut State, sha: String) -> Task<Message> {
    let Some(root) = root_of(state) else {
        return Task::none();
    };
    let (generation, stop) = state.loads.detail.begin();
    blocking(
        stop,
        move |cancelled| ncrs_git::commit_detail(&root, &sha, cancelled),
        move |result| Message::DetailLoaded(generation, Box::new(result)),
    )
}

pub fn working_changes(state: &mut State) -> Task<Message> {
    let Some(root) = root_of(state) else {
        return Task::none();
    };
    let (generation, stop) = state.loads.detail.begin();
    blocking(
        stop,
        move |cancelled| ncrs_git::working_changes(&root, cancelled),
        move |result| Message::WorkingLoaded(generation, result),
    )
}

pub fn file_diff(state: &mut State, target: DiffTarget, ignore_whitespace: bool) -> Task<Message> {
    let Some(root) = root_of(state) else {
        return Task::none();
    };
    let (generation, stop) = state.loads.diff.begin();
    blocking(
        stop,
        move |cancelled| ncrs_git::file_diff(&root, &target, ignore_whitespace, cancelled),
        move |result| Message::DiffLoaded(generation, Box::new(result)),
    )
}

/// Reads what the actions show; `None` in the answer's error means no actions here.
pub fn actions(state: &mut State) -> Task<Message> {
    let Some(root) = root_of(state) else {
        return Task::none();
    };
    let want = Want::of(state);
    let (generation, _) = state.loads.actions.begin();
    Task::perform(
        async move { tokio::task::spawn_blocking(move || snapshot::read(&root, &want)).await },
        move |joined| {
            Message::ActionsLoaded(
                generation,
                Box::new(joined.unwrap_or_else(|error| Err(ActionError::Io(error.to_string())))),
            )
        },
    )
}

/// Runs one call to git on the blocking pool.
pub fn job(root: PathBuf, job: Job) -> Task<Message> {
    let kept = job.clone();
    Task::perform(
        async move { tokio::task::spawn_blocking(move || job.run_in(&root)).await },
        move |joined| {
            Message::JobDone(Box::new(joined.unwrap_or_else(|error| Finished {
                job: kept,
                result: Err(ActionError::Io(error.to_string())),
            })))
        },
    )
}

/// Everything a change of the repository can touch.
pub fn everything(state: &mut State) -> Task<Message> {
    Task::batch([refs(state), log(state), actions(state)])
}

/// Reads which branches contain `sha`; it follows the detail, which does not wait for it.
pub fn containing(state: &mut State, sha: String) -> Task<Message> {
    let Some(root) = root_of(state) else {
        return Task::none();
    };
    let (generation, stop) = state.loads.containing.begin();
    let id = sha.clone();
    blocking(
        stop,
        move |cancelled| ncrs_git::branches_containing(&root, &sha, cancelled),
        move |result| Message::ContainingLoaded(generation, id.clone(), Box::new(result)),
    )
}
