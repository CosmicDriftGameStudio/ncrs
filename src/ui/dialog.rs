//! The modal prompt: a text field and two buttons over a dimmed background.
//!
//! Pure function, like every other component here: it takes the prompt state
//! and the language and returns an `Element`. It never mutates anything.

use iced::widget::{button, column, container, text, text_input, Row};
use iced::{alignment, Background, Element, Length};

use super::layout::DIALOG_WIDTH;
use super::theme::{self, colors, font_size, spacing};
use crate::dialog::Prompt;
use crate::i18n::{Language, Msg};
use crate::messages::Message;

/// Renders the prompt for `state`, or nothing when there is no prompt.
///
/// `on_input`, `on_submit` and `on_cancel` are passed in rather than built
/// here, so this component stays a view and the app decides the messages.
pub fn view<'a>(
    state: &'a Prompt,
    lang: Language,
    on_input: impl Fn(String) -> Message + 'a,
    on_submit: Message,
    on_cancel: Message,
) -> Option<Element<'a, Message>> {
    let (title, label, confirm) = match state {
        Prompt::CreateDir { .. } => (
            lang.text(Msg::DialogMkdirTitle),
            lang.text(Msg::DialogMkdirLabel),
            lang.text(Msg::DialogMkdirCreate),
        ),
    };

    let field = text_input(label, state.name())
        .on_input(on_input)
        .on_submit(on_submit.clone())
        .width(Length::Fill);

    // `on_press_maybe(None)` is how iced disables a button: no message, so a
    // keypress while the operation runs does nothing. There is no `disabled`.
    let submit = (!state.busy()).then_some(on_submit);
    let confirm_button = button(text(confirm))
        .on_press_maybe(submit)
        .style(theme::dialog_button(true));

    let cancel_button = button(text(lang.text(Msg::DialogCancel)))
        .on_press_maybe((!state.busy()).then_some(on_cancel))
        .style(theme::dialog_button(false));

    let mut body = column![
        text(title).size(font_size::TITLE).color(colors::ACCENT),
        container(field).padding([4.0, 0.0]),
        container(
            Row::with_children([confirm_button.into(), cancel_button.into()])
                .spacing(spacing::CELL_PADDING_X)
                .align_y(alignment::Vertical::Center),
        )
        .padding([4.0, 0.0]),
    ]
    .spacing(spacing::SECTION_GAP);

    if let Some(error) = state.error() {
        // A validation reason is a message id, not a sentence: the dialog layer
        // asked the user for something impossible and gets to word it.
        let message: &str = match validation_message(error) {
            Some(mid) => lang.text(mid),
            None => error,
        };
        body = body.push(
            text(message)
                .size(font_size::STATUS)
                .color(colors::ERROR)
                .wrapping(iced::widget::text::Wrapping::Word),
        );
    }
    Some(
        container(body)
            .padding(spacing::OUTER_PADDING * 2.0)
            .width(DIALOG_WIDTH)
            .style(theme::dialog)
            .into(),
    )
}

/// Maps a validation reason to its message, or `None` for a message the fs
/// layer already wrote in full.
fn validation_message(reason: &str) -> Option<Msg> {
    match reason {
        "empty_name" => Some(Msg::ErrorEmptyName),
        "reserved_name" => Some(Msg::ErrorReservedName),
        "separator_not_allowed" => Some(Msg::ErrorSeparatorNotAllowed),
        _ => None,
    }
}

/// A scrim behind the dialog, so clicks outside it do not reach the panels.
pub fn scrim<'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(Background::Color(colors::SCRIM)),
            ..container::Style::default()
        })
        .into()
}
