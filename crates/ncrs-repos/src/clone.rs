//! "Clone": the dialog that asks for an address and a folder, and the clone
//! that runs on the blocking pool. The new repository joins the list.

use std::path::PathBuf;

use iced::widget::operation;
use iced::Task;
use ncrs_i18n::{fill_template, Msg};

use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::State;
use crate::store::Origin;
use crate::update::{learn, Outcome};

/// The id of the address field of the dialog.
pub const CLONE_URL_ID: &str = "repos-clone-url";

/// What the dialog holds while it is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneDraft {
    pub url: String,
    /// The folder the clone gets its own folder in.
    pub target: PathBuf,
    pub running: bool,
    pub problem: Option<String>,
}

fn quiet(task: Task<Message>) -> Outcome {
    (task, Vec::new())
}

fn clone_task(url: String, target: PathBuf) -> Task<Message> {
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                ncrs_git::clone(&url, &target)
                    .map_err(|error| crate::bulk::error_line(&error))
            })
            .await
            .unwrap_or_else(|error| Err(error.to_string()))
        },
        Message::CloneDone,
    )
}

/// The folder the dialog starts with: the first folder searched, else home.
fn first_target(ctx: &Ctx) -> PathBuf {
    ctx.settings
        .sources
        .first()
        .cloned()
        .or_else(|| ctx.home.clone())
        .unwrap_or_default()
}

/// Handles the messages of the dialog; any other message comes back.
pub fn update(state: &mut State, message: Message, ctx: &Ctx) -> Result<Outcome, Message> {
    Ok(match message {
        Message::CloneOpen => {
            if !ctx.actions_available || state.clone.is_some() {
                return Ok(quiet(Task::none()));
            }
            state.clone = Some(CloneDraft {
                url: String::new(),
                target: first_target(ctx),
                running: false,
                problem: None,
            });
            state.filter_focused = false;
            quiet(operation::focus(CLONE_URL_ID))
        }
        Message::CloneUrlChanged(text) => {
            if let Some(draft) = state.clone.as_mut().filter(|draft| !draft.running) {
                draft.url = text;
                draft.problem = None;
            }
            quiet(Task::none())
        }
        Message::ClonePickTarget => (Task::none(), vec![Action::PickCloneTarget]),
        Message::CloneTargetPicked(folder) => {
            if let Some(draft) = state.clone.as_mut().filter(|draft| !draft.running) {
                draft.target = folder;
                draft.problem = None;
            }
            quiet(Task::none())
        }
        Message::CloneRun => {
            let Some(draft) = state.clone.as_mut().filter(|draft| !draft.running) else {
                return Ok(quiet(Task::none()));
            };
            if draft.url.trim().is_empty() {
                return Ok(quiet(Task::none()));
            }
            if !draft.target.is_dir() {
                draft.problem = Some(fill_template(
                    ctx.lang.text(Msg::ReposCloneTargetMissing),
                    &[("{folder}", &draft.target.to_string_lossy())],
                ));
                return Ok(quiet(Task::none()));
            }
            draft.running = true;
            draft.problem = None;
            quiet(clone_task(draft.url.clone(), draft.target.clone()))
        }
        Message::CloneDone(Ok(folder)) => {
            state.clone = None;
            let root = ncrs_git::repository_root_of(&folder).unwrap_or(folder);
            quiet(learn(state, root, Origin::Manual, ctx))
        }
        Message::CloneDone(Err(reason)) => {
            if let Some(draft) = state.clone.as_mut() {
                draft.running = false;
                draft.problem = Some(reason);
            }
            quiet(Task::none())
        }
        Message::CloneClose => {
            if state.clone.as_ref().is_some_and(|draft| !draft.running) {
                state.clone = None;
            }
            quiet(Task::none())
        }
        other => return Err(other),
    })
}
