//! The modal prompt: a text field and two buttons over a dimmed background.
//!
//! Pure function, like every other component here: it takes the prompt state
//! and the language and returns an `Element`. It never mutates anything.

use iced::widget::{column, container, text, text_input};
use iced::{alignment, Background, Element, Length, Size};

use super::frame::{self, DialogSize, Frame, KeyHints};
use super::theme::{self, font_size, spacing};
use crate::dialog::Prompt;
use crate::i18n::{Language, Msg};
use crate::messages::Message;
use crate::palette::Palette;

/// The text field carries an id, because `TextInput::id` exists and
/// `iced_selector` can find it. The buttons do not — see the note where they
/// are built.
pub const FIELD_ID: &str = "prompt-field";

/// Renders the prompt for `state`, or nothing when there is no prompt.
///
/// `on_input`, `on_submit` and `on_cancel` are passed in rather than built
/// here, so this component stays a view and the app decides the messages.
pub fn view<'a>(
    state: &'a Prompt,
    window: Size,
    lang: Language,
    palette: &Palette,
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
        .id(FIELD_ID)
        .on_input(on_input)
        .on_submit(on_submit.clone())
        .width(Length::Fill);

    // `on_press_maybe(None)` is how iced disables a button: no message, so a
    // keypress while the operation runs does nothing. There is no `disabled`.
    // The buttons carry no `widget::Id`: `Button::id` does not exist in iced
    // 0.14, and `iced_selector` can only find widgets by id. So they are found
    // by their bounds instead — see `ui_tests`, which clicks at the position
    // the layout reports for them. That is why the layout test and the click
    // test have to agree about where the buttons are.
    let idle = !state.busy();
    let confirm_button = frame::action(confirm, idle.then_some(on_submit), true, palette);
    let cancel_button = frame::action(
        lang.text(Msg::DialogCancel),
        idle.then(|| on_cancel.clone()),
        false,
        palette,
    );

    let mut body = column![container(field).padding([4.0, 0.0])].spacing(spacing::SECTION_GAP);

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
                .color(palette.error)
                .wrapping(iced::widget::text::Wrapping::Word),
        );
    }
    let mut framed = Frame::new(title, body, DialogSize::Small, window, lang, palette)
        .buttons(vec![cancel_button, confirm_button])
        .hints(KeyHints {
            enter: true,
            tab: false,
        });
    if idle {
        framed = framed.on_close(on_cancel);
    }
    Some(framed.into_element())
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
        .align_x(alignment::Horizontal::Center)
        .align_y(alignment::Vertical::Center)
        .style(|_theme| container::Style {
            background: Some(Background::Color(theme::SCRIM)),
            ..container::Style::default()
        })
        .into()
}
