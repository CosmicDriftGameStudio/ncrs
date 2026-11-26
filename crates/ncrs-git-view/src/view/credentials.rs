//! The dialog in which git asks for a login, from the kit.

use iced::Element;
use ncrs_git::askpass::{PromptKind, Secret};
use ncrs_ui_kit::credential_dialog::{CredentialDialog, CredentialKind};

use crate::credentials::CredentialAsk;
use crate::ctx::ViewCtx;
use crate::message::Message;

pub fn credential_dialog<'a>(ask: &'a CredentialAsk, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let id = ask.id;
    CredentialDialog {
        prompt: &ask.prompt,
        kind: match ask.kind {
            PromptKind::Text => CredentialKind::Text,
            PromptKind::Secret => CredentialKind::Secret,
            PromptKind::Confirm => CredentialKind::Confirm,
        },
        typed: ask.typed.expose(),
        on_input: move |typed: String| Message::CredentialTyped(id, Secret::new(typed)),
        send: Message::CredentialSend(id),
        cancel: Message::CredentialCancel(id),
    }
    .view(ctx.window, ctx.lang, ctx.palette)
}
