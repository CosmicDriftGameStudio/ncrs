//! The connect feature as the panels drive it: a folder on a server was asked
//! for without a connection, or the connection is closed.

use std::sync::Arc;

use iced::{widget::operation, Task};

use super::connect_failure_text;
use crate::action::Action;
use crate::ctx::Ctx;
use crate::dialog::{ConnectDialog, Stage, Then};
use crate::message::{ConnectField, Message};
use crate::state::Remote;
use crate::view::field_id;
use ncrs_core::PanelSide;
use ncrs_fs::remote::session::ConnectFailure;
use ncrs_fs::remote::{RemotePath, Secret, ServerId};
use ncrs_i18n::{fill_template, Msg};

/// The folder a panel asked for: the panel, the load it started, the place and
/// what to select in it.
pub type Asked = (PanelSide, u64, RemotePath, Option<String>);

/// Closes the connection. A panel that was on the server goes home.
pub fn disconnect_server(
    ctx: &Ctx<'_>,
    server: &ServerId,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    ctx.pool.forget(server);
    let text = fill_template(
        ctx.lang.text(Msg::StatusRemoteDisconnected),
        &[("{server}", &server.to_string())],
    );
    actions.push(Action::ShowNotice(text));
    actions.push(Action::LeaveServer(server.clone()));
    Task::none()
}

pub fn password_loaded(
    remote: &mut Remote,
    ctx: &Ctx<'_>,
    asked: Asked,
    stored: Result<Option<Secret>, String>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    let (side, request_id, place, select) = asked;
    if ctx.request_id(side) != request_id {
        return Task::none();
    }
    match stored {
        Ok(Some(password)) => {
            let pool = Arc::clone(ctx.pool);
            let server = place.server().clone();
            Task::perform(
                async move {
                    let outcome = pool.connect(&server, &password).await;
                    (password, outcome)
                },
                move |(kept, result)| Message::AutoConnected {
                    side,
                    request_id,
                    remote: place.clone(),
                    select: select.clone(),
                    password: kept,
                    result,
                },
            )
        }
        Ok(None) => ask_for_password(
            remote,
            ctx,
            (side, request_id, place, select),
            None,
            None,
            actions,
        ),
        Err(reason) => ask_for_password(
            remote,
            ctx,
            (side, request_id, place, select),
            None,
            Some(ConnectFailure::Keychain(reason)),
            actions,
        ),
    }
}

pub fn auto_connected(
    remote: &mut Remote,
    ctx: &Ctx<'_>,
    asked: Asked,
    password: Secret,
    result: Result<(), ConnectFailure>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    let (side, request_id, place, select) = asked;
    if ctx.request_id(side) != request_id {
        return Task::none();
    }
    match result {
        Ok(()) => {
            actions.push(Action::ListFolder {
                side,
                request_id,
                remote: place,
                select,
            });
            Task::none()
        }
        // A password that no longer fits is not kept, so that it is not
        // tried again with every folder, and not offered again.
        Err(ConnectFailure::AuthFailed) => {
            let forget = forget_remembered_password(ctx, place.server());
            let ask = ask_for_password(
                remote,
                ctx,
                (side, request_id, place, select),
                None,
                Some(ConnectFailure::AuthFailed),
                actions,
            );
            Task::batch([forget, ask])
        }
        Err(failure) => ask_for_password(
            remote,
            ctx,
            (side, request_id, place, select),
            Some(password),
            Some(failure),
            actions,
        ),
    }
}

/// Removes what the keychain holds for `server`, off the UI thread.
fn forget_remembered_password(ctx: &Ctx<'_>, server: &ServerId) -> Task<Message> {
    let passwords = Arc::clone(ctx.passwords);
    let gone = server.clone();
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || passwords.forget(&gone))
                .await
                .unwrap_or_else(|failure| Err(failure.to_string()))
        },
        Message::PasswordStored,
    )
}

/// Opens the connect dialog for the server of a folder that was asked for.
/// Another dialog being up, the panel is told it has no connection instead.
fn ask_for_password(
    remote: &mut Remote,
    ctx: &Ctx<'_>,
    asked: Asked,
    password: Option<Secret>,
    failure: Option<ConnectFailure>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    let (side, request_id, place, select) = asked;
    let server = place.server().clone();
    if ctx.blocking_overlay_open {
        actions.push(Action::ReportDisconnected {
            side,
            request_id,
            remote: place,
        });
        return Task::none();
    }
    let host_text = server.host.clone();
    let failure_text = failure
        .as_ref()
        .map(|reason| connect_failure_text(ctx.lang, reason, &host_text));
    let attempt = remote.next_attempt();
    let then = Then {
        side,
        path: place.to_path_buf(),
        select,
        request_id,
    };
    let mut dialog = ConnectDialog::for_server(attempt, &server, Some(then));
    if let Some(remembered) = password {
        dialog.keep_password(remembered);
    }
    match (failure, failure_text) {
        (Some(ConnectFailure::UnknownHost { fingerprint, key }), _) => {
            dialog.set_stage(Stage::HostKey { fingerprint, key });
        }
        (Some(_), Some(text)) => dialog.set_error(text),
        _ => {}
    }
    remote.dialog = Some(dialog);
    operation::focus(field_id(ConnectField::Password))
}
