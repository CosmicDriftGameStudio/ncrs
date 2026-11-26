//! The dialog in which git asks for a login: a user name, a password or
//! passphrase (shown as dots), or a yes or no.

use iced::widget::column;
use iced::{Element, Size};
use ncrs_core::palette::Palette;
use ncrs_i18n::{fill_template, Language, Msg};

use crate::format::ellipsize_end;
use crate::frame::{DialogSize, Frame};
use crate::kit::{button, dim, enter_button, paragraph, secret_field, text_field, Tone};
use crate::theme::spacing;

/// The id of the answer field.
pub const CREDENTIAL_ID: &str = "git-credential";

/// How many characters of git's own text the dialog shows.
const PROMPT_SHOWN: usize = 120;

/// What the question is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialKind {
    Text,
    Secret,
    Confirm,
}

/// The text git printed comes from a server or a helper: no control
/// characters (an escape sequence must not reach the screen), cut short.
pub fn shown_prompt(prompt: &str) -> String {
    let plain: String = prompt.chars().filter(|c| !c.is_control()).collect();
    ellipsize_end(plain.trim(), PROMPT_SHOWN)
}

fn title_of(kind: CredentialKind) -> Msg {
    match kind {
        CredentialKind::Text => Msg::GitViewCredentialsTitleText,
        CredentialKind::Secret => Msg::GitViewCredentialsTitleSecret,
        CredentialKind::Confirm => Msg::GitViewCredentialsTitleConfirm,
    }
}

/// What the dialog shows and what its buttons send.
pub struct CredentialDialog<'a, M, F> {
    pub prompt: &'a str,
    pub kind: CredentialKind,
    pub typed: &'a str,
    pub on_input: F,
    pub send: M,
    pub cancel: M,
}

impl<M, F> std::fmt::Debug for CredentialDialog<'_, M, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialDialog")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl<'a, M: Clone + 'a, F: Fn(String) -> M + 'a> CredentialDialog<'a, M, F> {
    pub fn view(
        self,
        window: Size,
        lang: Language,
        palette: &'a Palette,
    ) -> Element<'a, M> {
        let side_line = fill_template(
            lang.text(Msg::GitViewCredentialsAsk),
            &[("{prompt}", &shown_prompt(self.prompt))],
        );
        let mut content = column![].spacing(spacing::XS);
        match self.kind {
            CredentialKind::Text => {
                content = content
                    .push(paragraph(lang.text(Msg::GitViewCredentialsFieldText), palette))
                    .push(text_field(
                        "",
                        self.typed,
                        CREDENTIAL_ID,
                        Some(self.on_input),
                        self.send.clone(),
                        palette,
                    ));
            }
            CredentialKind::Secret => {
                content = content
                    .push(paragraph(
                        lang.text(Msg::GitViewCredentialsFieldSecret),
                        palette,
                    ))
                    .push(secret_field(
                        "",
                        self.typed,
                        CREDENTIAL_ID,
                        Some(self.on_input),
                        self.send.clone(),
                        palette,
                    ));
            }
            CredentialKind::Confirm => {
                content = content.push(paragraph(shown_prompt(self.prompt), palette));
            }
        }
        if self.kind != CredentialKind::Confirm {
            content = content.push(dim(side_line, palette));
        }
        content = content.push(dim(lang.text(Msg::GitViewCredentialsNote), palette));
        let cancel_label = lang.text(Msg::GitViewDialogCancel);
        let buttons = match self.kind {
            // Enter refuses an unknown host; only the button connects.
            CredentialKind::Confirm => vec![
                button(
                    lang.text(Msg::GitViewCredentialsConnect),
                    Some(self.send),
                    Tone::Secondary,
                    palette,
                ),
                enter_button(
                    cancel_label,
                    Some(self.cancel.clone()),
                    Tone::Primary,
                    palette,
                ),
            ],
            CredentialKind::Text | CredentialKind::Secret => vec![
                button(
                    cancel_label,
                    Some(self.cancel.clone()),
                    Tone::Secondary,
                    palette,
                ),
                enter_button(
                    lang.text(Msg::GitViewCredentialsSend),
                    (!self.typed.is_empty()).then_some(self.send),
                    Tone::Primary,
                    palette,
                ),
            ],
        };
        Frame::new(
            lang.text(title_of(self.kind)),
            content,
            DialogSize::Medium,
            window,
            lang,
            palette,
        )
        .buttons(buttons)
        .on_close(self.cancel)
        .into_element()
    }
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
