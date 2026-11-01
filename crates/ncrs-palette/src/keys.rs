//! What a key does while the command line has the focus.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

use crate::message::CommandLineMessage;

/// What a press is bound to, as far as the command line cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandLineBound {
    Quit,
    /// The key that focuses the command line.
    CommandLine,
    Other,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CommandLineKey {
    Message(CommandLineMessage),
    Quit,
}

/// Keys while the command line has the focus. The field types characters and
/// digits, so of the bindings only quitting and the key that focused the line
/// reach through, and only as keys the field does not type. Escape is
/// `InputState`'s; it arrives here only when the field lost its own focus.
/// `bound` is asked for a key the line does not know, and says `Other` for a
/// key a text field types.
pub fn command_line_key(
    key: &Key,
    modifiers: Modifiers,
    bound: impl FnOnce() -> CommandLineBound,
) -> Option<CommandLineKey> {
    let message = |message| Some(CommandLineKey::Message(message));
    match key.as_ref() {
        Key::Named(Named::Escape) => return message(CommandLineMessage::Leave),
        Key::Named(Named::ArrowUp) if modifiers.is_empty() => {
            return message(CommandLineMessage::HistoryPrevious)
        }
        Key::Named(Named::ArrowDown) if modifiers.is_empty() => {
            return message(CommandLineMessage::HistoryNext)
        }
        _ => {}
    }
    match bound() {
        CommandLineBound::Quit => Some(CommandLineKey::Quit),
        CommandLineBound::CommandLine => message(CommandLineMessage::Leave),
        CommandLineBound::Other => None,
    }
}
