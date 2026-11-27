//! The question before something is deleted: what goes, in a list, and two
//! buttons. It opens on Cancel, so Enter alone never deletes anything.

use iced::widget::column;
use iced::{Element, Size};
use ncrs_i18n::Language;

use crate::frame::{DialogSize, Frame, KeyHints};
use crate::kit::{self, Tone};
use crate::palette::Palette;
use crate::theme::spacing;

/// Which of the two buttons has the focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConfirmFocus {
    #[default]
    Cancel,
    Confirm,
}

/// A button with its label and, when it has one, the key shown on it.
#[derive(Debug, Clone, Copy)]
pub struct ConfirmButton<'a> {
    pub label: &'a str,
    pub key: Option<&'a str>,
}

pub struct Confirm<'a, M> {
    pub title: String,
    pub body: String,
    /// What goes: the names, a count, the sizes.
    pub list: Option<Element<'a, M>>,
    pub cancel: ConfirmButton<'a>,
    pub confirm: ConfirmButton<'a>,
    /// The look of the confirm button; `Tone::Danger` for a delete.
    pub tone: Tone,
    pub focus: ConfirmFocus,
    pub on_cancel: M,
    pub on_confirm: M,
    pub window: Size,
    pub lang: Language,
}

impl<M> std::fmt::Debug for Confirm<'_, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Confirm")
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

fn dialog_button<'a, M: Clone + 'a>(
    button: ConfirmButton<'_>,
    on_press: M,
    tone: Tone,
    palette: &Palette,
) -> Element<'a, M> {
    match button.key {
        Some(key) => kit::key_button(button.label, key, Some(on_press), tone, palette),
        None => kit::button(button.label, Some(on_press), tone, palette),
    }
}

pub fn confirm_dialog<'a, M: Clone + 'a>(
    confirm: Confirm<'a, M>,
    palette: &Palette,
) -> Element<'a, M> {
    let mut content = column![kit::paragraph(confirm.body, palette)].spacing(spacing::LINE_GAP);
    if let Some(list) = confirm.list {
        content = content.push(list);
    }
    let cancel_tone = Tone::filled_if(confirm.focus == ConfirmFocus::Cancel);
    Frame::new(
        confirm.title,
        content,
        DialogSize::Medium,
        confirm.window,
        confirm.lang,
        palette,
    )
    .buttons(vec![
        dialog_button(
            confirm.cancel,
            confirm.on_cancel.clone(),
            cancel_tone,
            palette,
        ),
        dialog_button(confirm.confirm, confirm.on_confirm, confirm.tone, palette),
    ])
    .on_close(confirm.on_cancel)
    .hints(KeyHints {
        enter: false,
        tab: true,
    })
    .into_element()
}
