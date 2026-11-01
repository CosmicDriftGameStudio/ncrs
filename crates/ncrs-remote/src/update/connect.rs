use std::sync::Arc;

use iced::{widget::operation, Task};

use super::connect_failure_text;
use crate::action::Action;
use crate::ctx::Ctx;
use crate::dialog::{ConnectDialog, Missing, Stage, Then};
use crate::message::{ConnectField, Message};
use crate::state::Remote;
use crate::view::field_id;
use ncrs_fs::remote::session::ConnectFailure;
use ncrs_fs::remote::RemotePath;
use ncrs_i18n::{fill_template, Msg};

pub fn submit(remote: &mut Remote, ctx: &Ctx<'_>) -> Task<Message> {
    let Some(dialog) = remote.dialog.as_ref() else {
        return Task::none();
    };
    if *dialog.stage() != Stage::Editing {
        return Task::none();
    }
    match dialog.target() {
        Ok(_) => start_connecting(remote, ctx),
        Err(missing) => operation::focus(field_id(match missing {
            Missing::Host => ConnectField::Host,
            Missing::Port => ConnectField::Port,
            Missing::User => ConnectField::User,
            Missing::Password => ConnectField::Password,
        })),
    }
}

/// Locks the dialog and connects with what it holds.
fn start_connecting(remote: &mut Remote, ctx: &Ctx<'_>) -> Task<Message> {
    let attempt = remote.next_attempt();
    let Some(dialog) = remote.dialog.as_mut() else {
        return Task::none();
    };
    let Ok((server, _)) = dialog.target() else {
        return Task::none();
    };
    dialog.next_attempt(attempt);
    dialog.set_stage(Stage::Connecting);
    let password = dialog.password().clone();
    let pool = Arc::clone(ctx.pool);
    Task::perform(
        async move { pool.connect(&server, &password).await },
        move |result| Message::Connected { attempt, result },
    )
}

pub fn cancel(remote: &mut Remote, actions: &mut Vec<Action>) -> Task<Message> {
    let Some(mut dialog) = remote.dialog.take() else {
        return Task::none();
    };
    // The folder that started the dialog is not coming: the panel stops
    // waiting for it.
    if let Some(then) = dialog.take_then() {
        actions.push(Action::StopWaiting {
            side: then.side,
            request_id: then.request_id,
        });
    }
    Task::none()
}

pub fn connection_made(
    remote: &mut Remote,
    ctx: &Ctx<'_>,
    attempt: u64,
    result: Result<(), ConnectFailure>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    if remote.dialog.as_ref().map(ConnectDialog::attempt) != Some(attempt) {
        return Task::none();
    }
    match result {
        Ok(()) => finish_connect(remote, ctx, actions),
        Err(failure) => {
            connect_failed(remote, ctx, failure);
            Task::none()
        }
    }
}

/// Back to editing with the reason, or on to the question about an unknown
/// server.
fn connect_failed(remote: &mut Remote, ctx: &Ctx<'_>, failure: ConnectFailure) {
    let host = remote
        .dialog
        .as_ref()
        .map(|dialog| dialog.value(ConnectField::Host).to_owned())
        .unwrap_or_default();
    let text = connect_failure_text(ctx.lang, &failure, &host);
    let Some(dialog) = remote.dialog.as_mut() else {
        return;
    };
    match failure {
        ConnectFailure::UnknownHost { fingerprint, key } => {
            dialog.set_stage(Stage::HostKey { fingerprint, key });
        }
        _ => {
            dialog.set_stage(Stage::Editing);
            dialog.set_error(text);
        }
    }
}

/// The connection is made: the dialog closes, the password goes to the
/// keychain when asked, and the panel opens the folder it was waiting for.
fn finish_connect(remote: &mut Remote, ctx: &Ctx<'_>, actions: &mut Vec<Action>) -> Task<Message> {
    let Some(mut dialog) = remote.dialog.take() else {
        return Task::none();
    };
    let Ok((server, typed_path)) = dialog.target() else {
        return Task::none();
    };
    let then = dialog.take_then();
    actions.push(Action::ClearJobError);
    let mut tasks = Vec::new();
    if dialog.save() {
        actions.push(Action::SaveServer {
            server: server.clone(),
            typed_path: typed_path.clone(),
        });
    }
    if dialog.remember() {
        let passwords = Arc::clone(ctx.passwords);
        let (stored_for, password) = (server.clone(), dialog.password().clone());
        tasks.push(Task::perform(
            async move {
                tokio::task::spawn_blocking(move || passwords.set(&stored_for, &password))
                    .await
                    .unwrap_or_else(|failure| Err(failure.to_string()))
            },
            Message::PasswordStored,
        ));
    }
    let home = ctx
        .pool
        .session(&server)
        .map_or_else(|| "/".to_owned(), |session| session.home().to_owned());
    let start = RemotePath::new(server.clone(), &home);
    let side = then
        .as_ref()
        .map_or(ctx.active_side, |waiting| waiting.side);
    let (target, select) = match (typed_path, then) {
        (Some(typed), _) => (start.resolve_typed(&typed, &home).to_path_buf(), None),
        (None, Some(Then { path, select, .. }))
            if RemotePath::parse(&path).is_some_and(|place| *place.server() == server) =>
        {
            (path, select)
        }
        _ => (start.to_path_buf(), None),
    };
    let text = fill_template(
        ctx.lang.text(Msg::StatusRemoteConnected),
        &[("{server}", &server.to_string())],
    );
    actions.push(Action::ShowNotice(text));
    actions.push(Action::LoadFolder {
        side,
        path: target,
        select,
    });
    Task::batch(tasks)
}

pub fn trust_host_key(remote: &mut Remote, ctx: &Ctx<'_>) -> Task<Message> {
    let Some(dialog) = remote.dialog.as_mut() else {
        return Task::none();
    };
    let Stage::HostKey { key, .. } = dialog.stage().clone() else {
        return Task::none();
    };
    let Ok((server, _)) = dialog.target() else {
        return Task::none();
    };
    remote.attempts = remote.attempts.wrapping_add(1);
    let attempt = remote.attempts;
    dialog.next_attempt(attempt);
    dialog.set_stage(Stage::Connecting);
    let pool = Arc::clone(ctx.pool);
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                pool.trust(&server, &key).map_err(|error| error.to_string())
            })
            .await
            .unwrap_or_else(|failure| Err(failure.to_string()))
        },
        move |result| Message::HostKeyLearned { attempt, result },
    )
}

pub fn host_key_learned(
    remote: &mut Remote,
    ctx: &Ctx<'_>,
    attempt: u64,
    result: Result<(), String>,
) -> Task<Message> {
    if remote.dialog.as_ref().map(ConnectDialog::attempt) != Some(attempt) {
        return Task::none();
    }
    match result {
        Ok(()) => start_connecting(remote, ctx),
        Err(reason) => {
            let text = fill_template(
                ctx.lang.text(Msg::ErrorRemoteFailed),
                &[("{reason}", &reason)],
            );
            if let Some(dialog) = remote.dialog.as_mut() {
                dialog.set_stage(Stage::Editing);
                dialog.set_error(text);
            }
            Task::none()
        }
    }
}
