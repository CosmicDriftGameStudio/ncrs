//! The dialog shown when a copy or move runs into a name that is taken.
//!
//! The "for all files" answer is the point of the dialog. Without it, copying
//! fifty files into a directory that has some of them means fifty identical
//! questions; with it, the user answers once and the rest follow. That was in
//! the requirements before anything here was built.

use iced::widget::{button, checkbox, column, container, text, Row};
use iced::{alignment, Element};

use super::layout::DIALOG_WIDTH;
use super::theme::{self, colors, font_size, spacing};
use crate::i18n::{Language, Msg};
use crate::messages::{ConflictChoice, Message};

/// Renders the conflict dialog.
///
/// `name` is the file that is in the way. `all` is whether "for all files" is
/// ticked; the app holds it so the tick survives the dialog being rebuilt every
/// frame.
pub fn view<'a>(
    name: &str,
    lang: Language,
    all: bool,
    on_choice: impl Fn(ConflictChoice) -> Message + 'a,
) -> Element<'a, Message> {
    let title = text(lang.text(Msg::ConflictTitle))
        .size(font_size::TITLE)
        .color(colors::ACCENT);

    let subject = text(lang.text(Msg::ConflictName).replace("{name}", name))
        .size(font_size::STATUS)
        .color(colors::TEXT);

    // The checkbox state comes from the app, not from here: the view is pure,
    // and the tick has to survive the dialog being rebuilt every frame.
    let all_row = Row::with_children([
        checkbox(all).style(theme::dialog_checkbox).into(),
        text(lang.text(Msg::ConflictApplyAll))
            .size(font_size::STATUS)
            .into(),
    ])
    .spacing(spacing::CELL_PADDING_X)
    .align_y(alignment::Vertical::Center);

    let button_row = Row::with_children([
        button(text(lang.text(Msg::ConflictOverwrite)))
            .on_press(on_choice(ConflictChoice::ThisOverwrite))
            .into(),
        button(text(lang.text(Msg::ConflictKeep)))
            .on_press(on_choice(ConflictChoice::ThisKeep))
            .into(),
        button(text(lang.text(Msg::ConflictCancel)))
            .on_press(on_choice(ConflictChoice::Cancel))
            .into(),
    ])
    .spacing(spacing::CELL_PADDING_X);

    container(column![title, subject, all_row, button_row].spacing(spacing::SECTION_GAP))
        .padding(spacing::OUTER_PADDING * 2.0)
        .width(DIALOG_WIDTH)
        .style(theme::dialog)
        .into()
}
