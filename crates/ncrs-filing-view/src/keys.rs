//! What a key does while the filing tab is up.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use ncrs_intake::LocationTransfer;

use crate::message::Message;
use crate::state::State;

/// What the keys read of the state of the tab.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct KeyFacts {
    /// A source is shown; without one the tab has no keys of its own.
    pub session: bool,
    pub question: bool,
    pub renaming: bool,
    pub link_marked: bool,
    /// Enter moves, as the source is set; Shift+Enter does the other.
    pub enter_moves: bool,
}

pub fn key_facts(state: &State) -> KeyFacts {
    state
        .session
        .as_ref()
        .map_or_else(KeyFacts::default, |session| KeyFacts {
            session: true,
            question: session.question.is_some(),
            renaming: session.renaming.is_some(),
            link_marked: session.marked_link.is_some(),
            enter_moves: session.source.transfer == LocationTransfer::Move,
        })
}

/// The message of a press. While a name is typed the text field has the keys;
/// Escape reaches it through the app's input state. An open question has all
/// keys.
pub fn filing_key(key: &Key, modifiers: Modifiers, facts: KeyFacts) -> Option<Message> {
    if !facts.session {
        return None;
    }
    if facts.question {
        return modifiers.is_empty().then(|| question_key(key)).flatten();
    }
    if facts.renaming {
        return None;
    }
    if modifiers == Modifiers::SHIFT && key.as_ref() == Key::Named(Named::Enter) {
        return Some(Message::File(transfer_of(!facts.enter_moves)));
    }
    if !modifiers.is_empty() {
        return None;
    }
    match key.as_ref() {
        Key::Named(Named::Enter) if facts.link_marked => Some(Message::PressMarkedLink),
        Key::Named(Named::Enter) => Some(Message::File(transfer_of(facts.enter_moves))),
        Key::Named(Named::ArrowUp) => Some(Message::SelectPrevious),
        Key::Named(Named::ArrowDown) => Some(Message::SelectNext),
        Key::Named(Named::ArrowLeft) => Some(Message::LinkPrevious),
        Key::Named(Named::ArrowRight) => Some(Message::LinkNext),
        Key::Named(Named::F2) => Some(Message::StartRename),
        Key::Named(Named::F6) => Some(Message::PickFolder),
        Key::Named(Named::F8) => Some(Message::Stay),
        _ => None,
    }
}

fn transfer_of(moves: bool) -> LocationTransfer {
    if moves {
        LocationTransfer::Move
    } else {
        LocationTransfer::Copy
    }
}

fn question_key(key: &Key) -> Option<Message> {
    match key.as_ref() {
        Key::Named(Named::Enter) => Some(Message::QuestionConfirm),
        Key::Named(Named::Escape) => Some(Message::QuestionLeave),
        Key::Named(Named::ArrowUp) => Some(Message::QuestionPrevious),
        Key::Named(Named::ArrowDown) => Some(Message::QuestionNext),
        Key::Named(Named::Space) => Some(Message::QuestionToggleMarked),
        _ => None,
    }
}
