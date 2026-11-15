//! What a key does while the filing tab is up.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

use crate::message::Message;

/// The message of a press. While a name is typed the text field has the keys;
/// Escape reaches the tab through the app's input state.
pub fn filing_key(key: &Key, modifiers: Modifiers, renaming: bool) -> Option<Message> {
    if renaming || !modifiers.is_empty() {
        return None;
    }
    match key.as_ref() {
        Key::Named(Named::ArrowUp) => Some(Message::SelectPrevious),
        Key::Named(Named::ArrowDown) => Some(Message::SelectNext),
        Key::Named(Named::F2) => Some(Message::StartRename),
        Key::Named(Named::F6) => Some(Message::PickFolder),
        Key::Named(Named::F8) => Some(Message::Stay),
        _ => None,
    }
}
