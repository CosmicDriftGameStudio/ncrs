//! Keyboard bindings: translate raw key presses into [`Message`]s.
//!
//! Keeping this mapping in one place makes it trivial to add shortcuts or,
//! later, to load them from a config file.

use iced::keyboard::{key::Named, Key, Modifiers};

use crate::messages::Message;

/// Shortcuts shown in the header bar: (key, description).
pub const SHORTCUTS: &[(&str, &str)] = &[
    ("Tab", "Switch Panel"),
    ("Enter", "Open"),
    ("Backspace", "Up"),
    ("F10", "Quit"),
];

/// Maps a key press to a message. Must be a plain `fn` for
/// `iced::keyboard::on_key_press`.
pub fn map_key(key: Key, _modifiers: Modifiers) -> Option<Message> {
    match key.as_ref() {
        Key::Named(Named::ArrowUp) => Some(Message::MoveSelection(-1)),
        Key::Named(Named::ArrowDown) => Some(Message::MoveSelection(1)),
        Key::Named(Named::PageUp) => Some(Message::PageUp),
        Key::Named(Named::PageDown) => Some(Message::PageDown),
        Key::Named(Named::Home) => Some(Message::SelectFirst),
        Key::Named(Named::End) => Some(Message::SelectLast),
        Key::Named(Named::Enter) => Some(Message::OpenSelected),
        Key::Named(Named::Backspace) => Some(Message::GoUp),
        Key::Named(Named::Tab) => Some(Message::SwitchPanel),
        Key::Named(Named::F10) => Some(Message::Quit),
        Key::Character("q") | Key::Character("Q") => Some(Message::Quit),
        _ => None,
    }
}
