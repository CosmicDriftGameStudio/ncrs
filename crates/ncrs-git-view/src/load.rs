//! The blocking reads of the repository, started on the blocking pool. A
//! newer read of the same kind stops the older one through its flag.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use iced::Task;
use ncrs_git::{DiffTarget, GitError};

use crate::message::Message;
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
        move |stop| ncrs_git::refs(&root, stop),
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
        move |stop| ncrs_git::log(&root, limit, &filter, stop),
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
        move |stop| ncrs_git::commit_detail(&root, &sha, stop),
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
        move |stop| ncrs_git::working_changes(&root, stop),
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
        move |stop| ncrs_git::file_diff(&root, &target, ignore_whitespace, stop),
        move |result| Message::DiffLoaded(generation, Box::new(result)),
    )
}
