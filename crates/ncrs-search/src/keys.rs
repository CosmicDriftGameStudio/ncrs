//! What a key does while the search view is up.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

use crate::message::SearchMessage;

#[derive(Debug, Clone, PartialEq)]
pub enum SearchKey {
    Message(SearchMessage),
    /// The key is bound to something that is not the search's: the app sends
    /// that binding on.
    Bound,
}

/// What the app resolved from the press before the search decides.
#[derive(Debug, Clone, Default)]
pub struct SearchKeyFacts {
    /// The search's version of the binding, when it is a tagging or toggle
    /// message.
    pub tag_or_toggle: Option<SearchMessage>,
    /// The press is bound to something and is not a key a text field types.
    pub bound_untyped: bool,
    /// The character a press types into the query, when it types one.
    pub type_ahead: Option<String>,
}

/// Keys while the search view is up. A text field captures what it types, so
/// what arrives here is either not text (arrows, function keys, chords) or the
/// field has no focus, and then the result list has the keys.
pub fn search_key(key: &Key, modifiers: Modifiers, facts: SearchKeyFacts) -> Option<SearchKey> {
    let message = |message| Some(SearchKey::Message(message));
    // Tagging and the toggles skip the filter below, which would drop Space,
    // `*` and the Alt chords as typing: a key only gets here when no text field
    // took it. Before the arrow match too, which ignores Shift and would turn
    // Shift+arrows into plain moves.
    if let Some(tagging) = facts.tag_or_toggle {
        return message(tagging);
    }
    if (modifiers - Modifiers::SHIFT).is_empty() {
        match key.as_ref() {
            Key::Named(Named::ArrowUp) => return message(SearchMessage::Move(-1)),
            Key::Named(Named::ArrowDown) => return message(SearchMessage::Move(1)),
            Key::Named(Named::PageUp) => return message(SearchMessage::Page(-1)),
            Key::Named(Named::PageDown) => return message(SearchMessage::Page(1)),
            Key::Named(Named::Home) => return message(SearchMessage::First),
            Key::Named(Named::End) => return message(SearchMessage::Last),
            Key::Named(Named::Enter) => return message(SearchMessage::Activate),
            _ => {}
        }
    }
    if facts.bound_untyped {
        return Some(SearchKey::Bound);
    }
    message(SearchMessage::AppendToQuery(facts.type_ahead?))
}
