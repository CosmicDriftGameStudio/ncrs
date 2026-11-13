//! What a key does while the notes tab is up.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

use crate::message::Message;

/// The message of a press. Escape is not here: the app sends
/// [`Message::Escape`] whatever field took the key. While a note is edited
/// only ⌘N counts, the arrow keys and Enter are the text's.
pub fn notes_key(key: &Key, modifiers: Modifiers, editing: bool) -> Option<Message> {
    if modifiers == Modifiers::COMMAND && key.as_ref() == Key::Character("n") {
        return Some(Message::NewNote);
    }
    if editing || !modifiers.is_empty() {
        return None;
    }
    match key.as_ref() {
        Key::Named(Named::ArrowUp) => Some(Message::Up),
        Key::Named(Named::ArrowDown) => Some(Message::Down),
        Key::Named(Named::Enter) => Some(Message::Enter),
        _ => None,
    }
}
