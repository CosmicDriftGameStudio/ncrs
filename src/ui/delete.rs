//! The confirmation dialog for F8 and Shift+F8.
//!
//! The two variants differ in more than wording. Moving to the trash can be
//! undone, so its default button, the one Enter presses, is the action. Deleting
//! for good cannot be, so there the default is "Cancel" and the dialog says why.

use iced::widget::{button, column, container, text, Row};
use iced::Element;

use super::layout::DIALOG_WIDTH;
use super::theme::{self, colors, font_size, spacing};
use crate::i18n::{Language, Msg};
use crate::messages::Message;

/// `focus` is the button Enter would press: 0 confirms, 1 cancels.
///
/// `subject` is the entry name or the count line, already worded by the caller
/// because it needs the number of entries.
pub fn view<'a>(
    subject: String,
    permanent: bool,
    focus: usize,
    lang: Language,
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

    let confirm = button(text(confirm_label))
        .on_press(Message::DeleteConfirm)
        .style(theme::dialog_button(focus == 0));
    let cancel = button(text(lang.text(Msg::DialogCancel)))
        .on_press(Message::DeleteCancel)
        .style(theme::dialog_button(focus == 1));

    let mut body = column![
        text(title).size(font_size::TITLE).color(colors::ACCENT),
        text(subject).size(font_size::STATUS).color(colors::TEXT),
    ]
    .spacing(spacing::SECTION_GAP);
    if permanent {
        body = body.push(
            text(lang.text(Msg::DeleteWarningPermanent))
                .size(font_size::STATUS)
                .color(colors::ERROR),
        );
    }
    body = body
        .push(Row::with_children([confirm.into(), cancel.into()]).spacing(spacing::CELL_PADDING_X));

    container(body)
        .padding(spacing::OUTER_PADDING * 2.0)
        .width(DIALOG_WIDTH)
        .style(theme::dialog)
        .into()
}
