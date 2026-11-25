//! The dialog in which git asks for a login: a user name, a password or
//! passphrase (shown as dots), or a yes or no.

use iced::widget::column;
use iced::Element;
use ncrs_git::askpass::{PromptKind, Secret};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::format::ellipsize_end;
use ncrs_ui_kit::frame::{DialogSize, Frame};
use ncrs_ui_kit::kit::{button, dim, enter_button, paragraph, secret_field, text_field, Tone};
use ncrs_ui_kit::theme::spacing;

use crate::credentials::CredentialAsk;
use crate::ctx::ViewCtx;
use crate::message::Message;

pub const CREDENTIAL_ID: &str = "git-credential";

/// How many characters of git's own text the dialog shows.
const PROMPT_SHOWN: usize = 120;

/// The text git printed comes from a server or a helper: no control
/// characters (an escape sequence must not reach the screen), cut short.
pub(crate) fn shown_prompt(prompt: &str) -> String {
    let plain: String = prompt.chars().filter(|c| !c.is_control()).collect();
    ellipsize_end(plain.trim(), PROMPT_SHOWN)
}

fn title_of(kind: PromptKind) -> Msg {
    match kind {
        PromptKind::Text => Msg::GitViewCredentialsTitleText,
        PromptKind::Secret => Msg::GitViewCredentialsTitleSecret,
        PromptKind::Confirm => Msg::GitViewCredentialsTitleConfirm,
    }
}

pub fn credential_dialog<'a>(ask: &'a CredentialAsk, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lang = ctx.lang;
    let side_line = fill_template(
        lang.text(Msg::GitViewCredentialsAsk),
        &[("{prompt}", &shown_prompt(&ask.prompt))],
    );
    let id = ask.id;
    let on_input = move |typed: String| Message::CredentialTyped(id, Secret::new(typed));
    let mut content = column![].spacing(spacing::XS);
    match ask.kind {
        PromptKind::Text => {
            content = content
                .push(paragraph(
                    lang.text(Msg::GitViewCredentialsFieldText),
                    palette,
                ))
                .push(text_field(
                    "",
                    ask.typed.expose(),
                    CREDENTIAL_ID,
                    Some(on_input),
                    Message::CredentialSend(id),
                    palette,
                ));
        }
        PromptKind::Secret => {
            content = content
                .push(paragraph(
                    lang.text(Msg::GitViewCredentialsFieldSecret),
                    palette,
                ))
                .push(secret_field(
                    "",
                    ask.typed.expose(),
                    CREDENTIAL_ID,
                    Some(on_input),
                    Message::CredentialSend(id),
                    palette,
                ));
        }
        PromptKind::Confirm => {
            content = content.push(paragraph(shown_prompt(&ask.prompt), palette));
        }
    }
    if ask.kind != PromptKind::Confirm {
        content = content.push(dim(side_line, palette));
    }
    content = content.push(dim(lang.text(Msg::GitViewCredentialsNote), palette));
    let cancel = Message::CredentialCancel(id);
    let cancel_label = lang.text(Msg::GitViewDialogCancel);
    let buttons = match ask.kind {
        // Enter refuses an unknown host; only the button connects.
        PromptKind::Confirm => vec![
            button(
                lang.text(Msg::GitViewCredentialsConnect),
                Some(Message::CredentialSend(id)),
                Tone::Secondary,
                palette,
            ),
            enter_button(cancel_label, Some(cancel.clone()), Tone::Primary, palette),
        ],
        PromptKind::Text | PromptKind::Secret => vec![
            button(cancel_label, Some(cancel.clone()), Tone::Secondary, palette),
            enter_button(
                lang.text(Msg::GitViewCredentialsSend),
                (!ask.typed.expose().is_empty()).then_some(Message::CredentialSend(id)),
                Tone::Primary,
                palette,
            ),
        ],
    };
    Frame::new(
        lang.text(title_of(ask.kind)),
        content,
        DialogSize::Medium,
        ctx.window,
        lang,
        palette,
    )
    .buttons(buttons)
    .on_close(cancel)
    .into_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_text_loses_control_characters_and_is_cut() {
        assert_eq!(shown_prompt("\u{1b}[2JPassword: \n"), "[2JPassword:");
        assert!(shown_prompt(&"x".repeat(500)).chars().count() <= PROMPT_SHOWN);
    }
}
