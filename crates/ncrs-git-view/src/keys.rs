//! What a key does while the Git tab is up.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

use crate::message::Message;

/// The message of a press. Escape is not here: the app sends
/// [`Message::Escape`] whatever field took the key. While the filter has the
/// keyboard only the arrows that leave it count.
pub fn git_view_key(
    key: &Key,
    modifiers: Modifiers,
    filter_focused: bool,
    in_diff: bool,
) -> Option<Message> {
    if modifiers == Modifiers::COMMAND && in_diff {
        return match key.as_ref() {
            Key::Named(Named::ArrowUp) => Some(Message::PreviousFile),
            Key::Named(Named::ArrowDown) => Some(Message::NextFile),
            _ => None,
        };
    }
    if filter_focused {
        return None;
    }
    // "/" is a shifted key on some layouts.
    let slash_modifiers = modifiers.is_empty() || modifiers == Modifiers::SHIFT;
    if slash_modifiers && key.as_ref() == Key::Character("/") {
        return Some(Message::FocusFilter);
    }
    if modifiers == Modifiers::SHIFT && key.as_ref() == Key::Named(Named::Tab) {
        return Some(Message::PreviousArea);
    }
    if !modifiers.is_empty() {
        return None;
    }
    match key.as_ref() {
        Key::Named(Named::ArrowUp) => Some(Message::Up),
        Key::Named(Named::ArrowDown) => Some(Message::Down),
        Key::Named(Named::ArrowLeft) => Some(Message::Left),
        Key::Named(Named::ArrowRight) => Some(Message::Right),
        Key::Named(Named::Enter) => Some(Message::Enter),
        Key::Named(Named::Tab) => Some(Message::NextArea),
        Key::Named(Named::F3) if in_diff => Some(Message::ViewFile),
        Key::Character("n") if in_diff => Some(Message::NextHunk),
        Key::Character("p") if in_diff => Some(Message::PreviousHunk),
        _ => None,
    }
}
