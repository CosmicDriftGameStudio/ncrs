//! The state changes of the connect feature. What reaches outside them comes
//! back as actions.

mod connect;
mod failure_text;
mod folder;

use iced::{widget::operation, Task};

use crate::action::Action;
use crate::ctx::Ctx;
use crate::dialog::ConnectDialog;
use crate::message::{ConnectField, Message};
use crate::state::Remote;
use crate::view::field_id;
use ncrs_fs::remote::RemoteUrl;
use ncrs_i18n::{fill_template, Msg};

pub use self::failure_text::connect_failure_text;

pub type Outcome = (Task<Message>, Vec<Action>);

pub fn update(remote: &mut Remote, message: Message, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = match message {
        Message::OpenConnect => open_connect_dialog(remote, ctx, &mut actions),
        Message::Field(field, text) => {
            if let Some(dialog) = remote.dialog.as_mut() {
                dialog.set(field, text);
            }
            Task::none()
        }
        Message::Password(password) => {
            if let Some(dialog) = remote.dialog.as_mut() {
                dialog.set_password(password);
            }
            Task::none()
        }
        Message::ToggleRemember => {
            if let Some(dialog) = remote.dialog.as_mut() {
                dialog.toggle_remember();
            }
            Task::none()
        }
        Message::ToggleSave => {
            if let Some(dialog) = remote.dialog.as_mut() {
                dialog.toggle_save();
            }
            Task::none()
        }
        Message::OpenAdd => {
            let opened = open_connect_dialog(remote, ctx, &mut actions);
            if let Some(dialog) = remote.dialog.take() {
                remote.dialog = Some(dialog.with_save(true));
            }
            opened
        }
        Message::OpenServer(url) => {
            actions.push(Action::OpenServer(url));
            Task::none()
        }
        Message::FocusField { backwards } => {
            if backwards {
                operation::focus_previous()
            } else {
                operation::focus_next()
            }
        }
        Message::Submit => connect::submit(remote, ctx),
        Message::Cancel => connect::cancel(remote, &mut actions),
        Message::Connected { attempt, result } => {
            connect::connection_made(remote, ctx, attempt, result, &mut actions)
        }
        Message::TrustHostKey => connect::trust_host_key(remote, ctx),
        Message::HostKeyLearned { attempt, result } => {
            connect::host_key_learned(remote, ctx, attempt, result)
        }
        Message::DisconnectActive => match ctx.active_panel_server.clone() {
            Some(server) => folder::disconnect_server(ctx, &server, &mut actions),
            None => {
                let text = ctx.lang.text(Msg::StatusRemoteNotConnected).to_owned();
                actions.push(Action::ShowNotice(text));
                Task::none()
            }
        },
        Message::Disconnect(server) => folder::disconnect_server(ctx, &server, &mut actions),
        Message::PasswordLoaded {
            side,
            request_id,
            remote: place,
            select,
            result,
        } => folder::password_loaded(
            remote,
            ctx,
            (side, request_id, place, select),
            result,
            &mut actions,
        ),
        Message::AutoConnected {
            side,
            request_id,
            remote: place,
            select,
            password,
            result,
        } => folder::auto_connected(
            remote,
            ctx,
            (side, request_id, place, select),
            password,
            result,
            &mut actions,
        ),
        Message::PasswordStored(result) => {
            if let Err(reason) = result {
                let text = fill_template(
                    ctx.lang.text(Msg::ErrorRemoteKeychain),
                    &[("{reason}", &reason)],
                );
                actions.push(Action::ShowError(text));
            }
            Task::none()
        }
    };
    (task, actions)
}

/// The connect dialog for an address that names no user.
pub fn open_connect_dialog_for(remote: &mut Remote, ctx: &Ctx<'_>, url: &RemoteUrl) -> Outcome {
    if ctx.blocking_overlay_open {
        return (Task::none(), Vec::new());
    }
    let attempt = remote.next_attempt();
    remote.dialog = Some(ConnectDialog::for_address(attempt, url));
    (operation::focus(field_id(ConnectField::User)), Vec::new())
}

fn open_connect_dialog(
    remote: &mut Remote,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    if ctx.blocking_overlay_open {
        return Task::none();
    }
    actions.push(Action::CloseContextMenu);
    let attempt = remote.next_attempt();
    remote.dialog = Some(ConnectDialog::new(attempt));
    operation::focus(field_id(ConnectField::Host))
}
