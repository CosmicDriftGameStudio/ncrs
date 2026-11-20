//! What a key does while the notes tab is up.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

use crate::message::{Message, Mode};

/// The message of a press. Escape is not here: the app sends
/// [`Message::Escape`] whatever field took the key. While a note is edited
/// only the chords with ⌘ count; the arrow keys, Enter and the plain keys are
/// the text's, except F7.
pub fn notes_key(key: &Key, modifiers: Modifiers, editing: bool) -> Option<Message> {
    if modifiers == Modifiers::COMMAND {
        return match key.as_ref() {
            Key::Character("n") => Some(Message::NewNote),
            Key::Character("l") => Some(Message::ToggleLock),
            Key::Character("f") => Some(Message::FocusSearch),
            _ => None,
        };
    }
    if modifiers.is_empty() {
        return match key.as_ref() {
            Key::Named(Named::F7) => Some(Message::NewFolder),
            _ if editing => None,
            Key::Named(Named::ArrowUp) => Some(Message::Up),
            Key::Named(Named::ArrowDown) => Some(Message::Down),
            Key::Named(Named::ArrowLeft) => Some(Message::Left),
            Key::Named(Named::ArrowRight) => Some(Message::Right),
            Key::Named(Named::Enter) => Some(Message::Enter),
            Key::Named(Named::F4) => Some(Message::SetMode(Mode::Edit)),
            Key::Character("/") => Some(Message::FocusSearch),
            _ => None,
        };
    }
    // "/" needs Shift on some layouts.
    (modifiers == Modifiers::SHIFT && !editing && key.as_ref() == Key::Character("/"))
        .then_some(Message::FocusSearch)
}
