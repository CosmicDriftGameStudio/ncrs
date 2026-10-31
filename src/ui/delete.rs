//! The confirmation dialog for F8 and Shift+F8.
//!
//! The two variants differ in more than wording. Moving to the trash can be
//! undone, so its default button, the one Enter presses, is the action. Deleting
//! for good cannot be, so there the default is "Cancel" and the dialog says why.

use iced::widget::text::Wrapping;
use iced::widget::{column, text, Column};
use iced::{Element, Size};

use super::format;
use super::frame::{self, DialogSize, Frame, KeyHints};
use super::kit::{self, Tone};
use super::theme::{font_size, spacing};
use crate::i18n::{FuturamaName, Language, Msg};
use crate::messages::Message;
use crate::palette::Palette;

const SIZE: DialogSize = DialogSize::Medium;

/// The buttons from left to right: cancel is `0`, confirm is `1`, rightmost.
pub const CANCEL_FOCUS: usize = 0;
pub const CONFIRM_FOCUS: usize = 1;

/// The button Enter presses when the dialog opens. The permanent dialog starts
/// on Cancel, so a reflex Enter never deletes for good.
pub const fn default_focus(permanent: bool) -> usize {
    if permanent {
        CANCEL_FOCUS
    } else {
        CONFIRM_FOCUS
    }
}

/// `focus` is the button Enter would press.
///
/// `subject` is the entry name or the count line, already worded by the caller
/// because it needs the number of entries.
pub fn view<'a>(
    subject: &str,
    permanent: bool,
    focus: usize,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let (title, confirm_label) = if permanent {
        (
            lang.text(Msg::DeleteTitlePermanent),
            lang.text(Msg::DeleteConfirmPermanent),
        )
    } else {
        (
            lang.text(Msg::DeleteTitleTrash),
            lang.text(Msg::DeleteConfirmTrash),
        )
    };

    let line_chars = frame::content_chars(SIZE, window, 0.0);
    let names = Column::with_children(subject.lines().map(|line| {
        text(format::ellipsize_middle(line, line_chars))
            .size(font_size::BODY)
            .color(palette.text)
            .wrapping(Wrapping::None)
            .into()
    }))
    .spacing(spacing::XS / 2.0);
    let mut content = column![names].spacing(spacing::LINE_GAP);
    if permanent {
        content = content.push(kit::error_line(
            lang.text(Msg::DeleteWarningPermanent),
            palette,
        ));
    }

    let mut frame = Frame::new(title, content, SIZE, window, lang, palette);
    if !permanent {
        frame = frame.title_theme(lang.futurama_name(FuturamaName::ActionDelete));
    }
    frame
        .buttons(vec![
            if focus == CANCEL_FOCUS && !permanent {
                kit::enter_button(
                    lang.text(Msg::DialogCancel),
                    Some(Message::DeleteCancel),
                    Tone::Primary,
                    palette,
                )
            } else {
                kit::button(
                    lang.text(Msg::DialogCancel),
                    Some(Message::DeleteCancel),
                    Tone::filled_if(focus == CANCEL_FOCUS),
                    palette,
                )
            },
            if focus == CONFIRM_FOCUS && !permanent {
                kit::enter_button(
                    confirm_label,
                    Some(Message::DeleteConfirm),
                    Tone::Danger,
                    palette,
                )
            } else {
                kit::button(
                    confirm_label,
                    Some(Message::DeleteConfirm),
                    Tone::Danger,
                    palette,
                )
            },
        ])
        .on_close(Message::DeleteCancel)
        // The permanent dialog starts on Cancel, so Enter does not confirm.
        .hints(KeyHints {
            enter: !permanent,
            tab: true,
        })
        .into_element()
}
