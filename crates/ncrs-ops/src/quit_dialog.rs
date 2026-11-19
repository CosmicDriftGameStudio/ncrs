//! The question when the app is asked to quit while a job still runs: stop the
//! job and leave, or let it finish.
//!
//! Cancel has the focus when the dialog opens, so a reflex Enter never stops
//! a job that is half done.

use iced::widget::text::Wrapping;
use iced::widget::{column, text};
use iced::{Element, Size};

use ncrs_i18n::{Language, Msg};
use ncrs_ui_kit::frame::{DialogSize, Frame, KeyHints};
use ncrs_ui_kit::kit::{self, Tone};
use ncrs_ui_kit::palette::Palette;
use ncrs_ui_kit::theme::{font_size, spacing};

use crate::message::OpsMessage;

const SIZE: DialogSize = DialogSize::Medium;

/// The buttons from left to right: cancel is `0`, confirm is `1`, rightmost.
pub const CANCEL_FOCUS: usize = 0;
pub const CONFIRM_FOCUS: usize = 1;

/// `focus` is the button Enter would press.
pub fn view<'a>(
    focus: usize,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, OpsMessage> {
    let content = column![text(lang.text(Msg::QuitBusyHint).to_owned())
        .size(font_size::BODY)
        .color(palette.text)
        .wrapping(Wrapping::Word)]
    .spacing(spacing::LINE_GAP);
    let button = |label: &str, message: OpsMessage, own_focus: usize| {
        if focus == own_focus {
            kit::enter_button(label, Some(message), Tone::Primary, palette)
        } else {
            kit::button(label, Some(message), Tone::Secondary, palette)
        }
    };
    Frame::new(
        lang.text(Msg::QuitBusyTitle),
        content,
        SIZE,
        window,
        lang,
        palette,
    )
    .buttons(vec![
        button(
            lang.text(Msg::QuitBusyKeep),
            OpsMessage::QuitCancel,
            CANCEL_FOCUS,
        ),
        button(
            lang.text(Msg::QuitBusyConfirm),
            OpsMessage::QuitConfirm,
            CONFIRM_FOCUS,
        ),
    ])
    .on_close(OpsMessage::QuitCancel)
    .hints(KeyHints {
        enter: true,
        tab: true,
    })
    .into_element()
}
