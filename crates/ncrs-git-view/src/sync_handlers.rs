//! The menus behind Pull and Push, the force push, the login dialog of git and
//! the fetch in the background.

use iced::Task;
use ncrs_git::askpass::{AskRequest, Secret};
use ncrs_git::ForcePushPlan;

use crate::actions::{Dialog, Question};
use crate::credentials::{self, CredentialAsk};
use crate::ctx::Ctx;
use crate::handlers::start;
use crate::job::Job;
use crate::load;
use crate::state::State;
use crate::sync_menu::{SyncAction, SyncKind, SyncMenu};
use crate::update::{quiet, Outcome};

/// The arrow of Pull or Push, or its key: opens the menu, or closes it when it is the open one.
/// The cursor of the Pull menu starts on the way the Pull button takes.
pub fn toggle_menu(state: &mut State, kind: SyncKind, ctx: &Ctx) -> Outcome {
    let open = state.actions.sync_menu.take();
    if state.actions.unavailable.is_some() || open.is_some_and(|menu| menu.kind == kind) {
        return quiet(Task::none());
    }
    let mut menu = SyncMenu::new(kind);
    if kind == SyncKind::Pull {
        let default = SyncAction::Pull(ctx.pull_default);
        menu.cursor = kind
            .entries()
            .iter()
            .position(|entry| *entry == default)
            .unwrap_or_default();
    }
    state.actions.menu = None;
    state.actions.sync_menu = Some(menu);
    quiet(Task::none())
}

pub fn close_menu(state: &mut State) {
    state.actions.sync_menu = None;
}

pub fn hover(state: &mut State, entry: usize) -> Outcome {
    if let Some(menu) = state.actions.sync_menu.as_mut() {
        menu.cursor = entry;
    }
    quiet(Task::none())
}

pub fn step_menu(state: &mut State, down: bool) -> Outcome {
    if let Some(menu) = state.actions.sync_menu.as_mut() {
        menu.step(down);
    }
    quiet(Task::none())
}

pub fn run_cursor(state: &mut State) -> Outcome {
    match state.actions.sync_menu.and_then(|menu| menu.under_cursor()) {
        Some(action) => run_entry(state, action),
        None => quiet(Task::none()),
    }
}

/// A letter picks its entry; a letter no entry has changes nothing.
pub fn letter(state: &mut State, typed: char) -> Outcome {
    match state
        .actions
        .sync_menu
        .and_then(|menu| menu.with_letter(typed))
    {
        Some(action) => run_entry(state, action),
        None => quiet(Task::none()),
    }
}

pub fn run_entry(state: &mut State, action: SyncAction) -> Outcome {
    state.actions.sync_menu = None;
    match action {
        SyncAction::Pull(strategy) => start(state, Job::Pull(Some(strategy))),
        SyncAction::Push => start(state, Job::Push),
        SyncAction::ForcePush => force_push(state),
    }
}

/// Reads what the force push would replace; the question comes with the answer.
pub fn force_push(state: &mut State) -> Outcome {
    state.actions.sync_menu = None;
    if state
        .actions
        .sync
        .as_ref()
        .is_some_and(|sync| sync.force_blocked.is_some())
    {
        return quiet(Task::none());
    }
    start(state, Job::PrepareForcePush)
}

pub fn ask_force_push(state: &mut State, plan: ForcePushPlan) -> Outcome {
    state.actions.dialog = Some(Dialog::Ask(Question::ForcePush { plan }));
    quiet(Task::none())
}

/// Git asked for a login in a job of this tab.
pub fn credential_asked(state: &mut State, request: AskRequest) -> Outcome {
    if state.actions.dialog.is_none() {
        state.actions.dialog = Some(Dialog::Credential(CredentialAsk::of(&request)));
    } else {
        state.actions.waiting_credentials.push(request);
    }
    quiet(Task::none())
}

pub fn credential_typed(state: &mut State, id: u64, typed: Secret) -> Outcome {
    if let Some(Dialog::Credential(ask)) = state.actions.dialog.as_mut() {
        if ask.id == id {
            ask.typed = typed;
        }
    }
    quiet(Task::none())
}

/// A login question that came while another dialog was open.
pub(crate) fn show_next_credential(state: &mut State) {
    if !state.actions.waiting_credentials.is_empty() {
        let next = state.actions.waiting_credentials.remove(0);
        state.actions.dialog = Some(Dialog::Credential(CredentialAsk::of(&next)));
    }
}

/// Enter in the login dialog. A yes or no question (an unknown host key) is
/// refused by Enter: it is the safe answer, "Connect" is the button's.
pub fn credential_enter(state: &mut State) -> Outcome {
    let Some(Dialog::Credential(ask)) = state.actions.dialog.as_ref() else {
        return quiet(Task::none());
    };
    if ask.kind == ncrs_git::askpass::PromptKind::Confirm {
        cancel_credential(state);
        return quiet(Task::none());
    }
    let id = ask.id;
    send_credential(state, id)
}

/// The button of the login dialog for question `id`; a press meant for
/// another question than the shown one is dropped. An empty answer to a
/// question for text stays unsent.
pub fn send_credential(state: &mut State, id: u64) -> Outcome {
    let Some(Dialog::Credential(ask)) = state.actions.dialog.as_ref() else {
        return quiet(Task::none());
    };
    if ask.id != id {
        return quiet(Task::none());
    }
    let is_text = ask.kind != ncrs_git::askpass::PromptKind::Confirm;
    if is_text && ask.typed.expose().is_empty() {
        return quiet(Task::none());
    }
    ncrs_git::askpass::answer(ask.id, Some(ask.answer()));
    state.actions.dialog = None;
    show_next_credential(state);
    quiet(Task::none())
}

/// Esc, "Cancel" or a "no": git is told nobody answered. `true` when it was the login dialog.
pub fn cancel_credential(state: &mut State) -> bool {
    let Some(Dialog::Credential(ask)) = state.actions.dialog.take() else {
        return false;
    };
    credentials::refuse(ask.id);
    show_next_credential(state);
    true
}

/// The Cancel button for question `id`.
pub fn cancel_credential_of(state: &mut State, id: u64) -> Outcome {
    if matches!(&state.actions.dialog, Some(Dialog::Credential(ask)) if ask.id == id) {
        cancel_credential(state);
    }
    quiet(Task::none())
}

/// The job that asked is over: nobody waits for an answer any more.
pub fn end_of_job(state: &mut State) {
    if matches!(state.actions.dialog, Some(Dialog::Credential(_))) {
        state.actions.dialog = None;
    }
    for request in state.actions.waiting_credentials.drain(..) {
        credentials::refuse(request.id);
    }
}

/// The timer of the background fetch. A tab that does something, shows a
/// dialog or cannot write skips this round.
pub fn background_fetch(state: &mut State) -> Outcome {
    let idle = state.actions.deliberate_sync
        && state.actions.busy.is_none()
        && state.actions.unavailable.is_none()
        && !state.actions.background_fetch
        && !state.actions.dialog_open();
    let Some(root) = state.root.clone().filter(|_| idle) else {
        return quiet(Task::none());
    };
    state.actions.background_fetch = true;
    quiet(load::job(root, state.job_token, Job::BackgroundFetch))
}

/// The background fetch is over; errors stay silent. What the user started meanwhile starts now.
pub fn background_done(state: &mut State, fetched: bool) -> Outcome {
    state.actions.background_fetch = false;
    let reload = if fetched {
        load::everything(state)
    } else {
        Task::none()
    };
    match state.actions.queued.take() {
        Some(job) => {
            let (started, action) = start(state, job);
            (Task::batch([reload, started]), action)
        }
        None => quiet(reload),
    }
}
