//! What a key does while the viewer tab is up.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

use crate::message::{Message, Scroll};

/// The message of a press, for a tab that shows Markdown or not. Keys the
/// viewer does not know go on to the app.
pub fn viewer_key(key: &Key, modifiers: Modifiers, markdown: bool) -> Option<Message> {
    if !modifiers.is_empty() {
        return None;
    }
    let scroll = |scroll| Some(Message::Scroll(scroll));
    match key.as_ref() {
        Key::Named(Named::Escape) => Some(Message::Close),
        Key::Named(Named::F4) => Some(Message::Edit),
        Key::Named(Named::Tab) if markdown => Some(Message::ToggleMode),
        Key::Named(Named::ArrowUp) => scroll(Scroll::Lines(-1)),
        Key::Named(Named::ArrowDown) => scroll(Scroll::Lines(1)),
        Key::Named(Named::PageUp) => scroll(Scroll::Pages(-1)),
        Key::Named(Named::PageDown) => scroll(Scroll::Pages(1)),
        Key::Named(Named::Home) => scroll(Scroll::Top),
        Key::Named(Named::End) => scroll(Scroll::End),
        _ => None,
    }
}
