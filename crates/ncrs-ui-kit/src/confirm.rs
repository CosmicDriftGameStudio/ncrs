//! The question before something is deleted: what goes, in a list, and two
//! buttons. It opens on Cancel, so Enter alone never deletes anything.

use iced::widget::column;
use iced::{Element, Size};
use ncrs_i18n::Language;

use crate::frame::{DialogSize, Frame, KeyHints};
use crate::kit::{self, Tone};
use crate::palette::Palette;
use crate::theme::spacing;
use crate::theme::{self, focus_border};
use iced::widget::container;

/// Which of the two buttons has the focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConfirmFocus {
    #[default]
    Cancel,
    Confirm,
}

/// A button with its label and, when it has one, the key shown on it.
#[derive(Debug, Clone)]
pub struct ConfirmButton<'a> {
    pub label: &'a str,
    pub key: Option<String>,
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
    pub size: DialogSize,
    /// Under the buttons instead of the keys the frame names.
    pub hint: Option<String>,
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
        Some(key) => kit::key_button(button.label, &key, Some(on_press), tone, palette),
        None => kit::button(button.label, Some(on_press), tone, palette),
    }
}

/// The 1 px focus outline around the button that has the keys; the other keeps the
/// room of the outline, so the buttons do not move.
fn focus_ring<'a, M: 'a>(
    button: Element<'a, M>,
    focused: bool,
    palette: &Palette,
) -> Element<'a, M> {
    let ring = focus_border(palette);
    container(button)
        .padding(theme::spacing::BORDER_WIDTH)
        .style(move |_theme: &iced::Theme| container::Style {
            border: if focused {
                ring
            } else {
                iced::Border {
                    color: iced::Color::TRANSPARENT,
                    ..ring
                }
            },
            ..container::Style::default()
        })
        .into()
}

pub fn confirm_dialog<'a, M: Clone + 'a>(
    confirm: Confirm<'a, M>,
    palette: &Palette,
) -> Element<'a, M> {
    let mut content = column![kit::paragraph(confirm.body, palette)].spacing(spacing::LINE_GAP);
    if let Some(list) = confirm.list {
        content = content.push(list);
    }
    let cancel = focus_ring(
        dialog_button(
            confirm.cancel,
            confirm.on_cancel.clone(),
            Tone::Secondary,
            palette,
        ),
        confirm.focus == ConfirmFocus::Cancel,
        palette,
    );
    let accept = focus_ring(
        dialog_button(confirm.confirm, confirm.on_confirm, confirm.tone, palette),
        confirm.focus == ConfirmFocus::Confirm,
        palette,
    );
    let mut frame = Frame::new(
        confirm.title,
        content,
        confirm.size,
        confirm.window,
        confirm.lang,
        palette,
    )
    .buttons(vec![cancel, accept])
    .on_close(confirm.on_cancel)
    .hints(KeyHints {
        enter: false,
        tab: true,
    });
    if let Some(hint) = confirm.hint {
        frame = frame.footer_note(hint);
    }
    frame.into_element()
}
