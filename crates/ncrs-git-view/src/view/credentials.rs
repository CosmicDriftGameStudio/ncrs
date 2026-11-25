//! The dialog in which git asks for a login: a user name, a password or
//! passphrase (shown as dots), or a yes or no.

use iced::widget::column;
use iced::Element;
use ncrs_git::askpass::{PromptKind, Secret};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::kit::{dim, paragraph, secret_field, text_field, Tone};
use ncrs_ui_kit::theme::spacing;

use super::overlay::dialog_frame;
use crate::credentials::CredentialAsk;
use crate::ctx::ViewCtx;
use crate::message::Message;

pub const CREDENTIAL_ID: &str = "git-credential";

pub fn credential_dialog<'a>(ask: &'a CredentialAsk, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lang = ctx.lang;
    let question = fill_template(
        lang.text(Msg::GitViewCredentialsAsk),
        &[("{prompt}", ask.prompt.trim())],
    );
    let on_input = |typed: String| Message::CredentialTyped(Secret::new(typed));
    let mut content = column![paragraph(question, palette)].spacing(spacing::XS);
    match ask.kind {
        PromptKind::Text => {
            content = content.push(text_field(
                "",
                ask.typed.expose(),
                CREDENTIAL_ID,
                Some(on_input),
                Message::ConfirmDialog,
                palette,
            ));
        }
        PromptKind::Secret => {
            content = content.push(secret_field(
                "",
                ask.typed.expose(),
                CREDENTIAL_ID,
                Some(on_input),
                Message::ConfirmDialog,
                palette,
            ));
        }
        PromptKind::Confirm => {}
    }
    content = content.push(dim(lang.text(Msg::GitViewCredentialsNote), palette));
    let (confirm, can_send) = match ask.kind {
        PromptKind::Confirm => (Msg::GitViewCredentialsConnect, true),
        PromptKind::Text | PromptKind::Secret => (
            Msg::GitViewCredentialsSend,
            !ask.typed.expose().is_empty(),
        ),
    };
    dialog_frame(
        lang.text(Msg::GitViewCredentialsTitle),
        content.into(),
        lang.text(confirm),
        can_send.then_some(Message::ConfirmDialog),
        Tone::Primary,
        ctx,
    )
}
