//! What a key does in the small Hermes cards. The pages of Hermes have no keys
//! of their own: they are pages of the settings and use its keys.

use iced::keyboard::key::Named;
use iced::keyboard::Key;

use crate::message::{HermesOverviewMessage, Message, QuickFixMessage};

/// What the keys read of the state of the app.
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyFacts {
    pub start_over_dialog: bool,
}

/// The quick-fix menu takes the arrows and Enter; any other key does nothing
/// until it is closed.
pub fn quick_fix_key(key: &Key) -> Option<Message> {
    let fix = |message| Some(Message::QuickFix(message));
    match key.as_ref() {
        Key::Named(Named::Escape) => fix(QuickFixMessage::Close),
        Key::Named(Named::Enter) => fix(QuickFixMessage::Activate),
        Key::Named(Named::ArrowUp) => fix(QuickFixMessage::Move(-1)),
        Key::Named(Named::ArrowDown) => fix(QuickFixMessage::Move(1)),
        Key::Named(Named::Home) => fix(QuickFixMessage::First),
        Key::Named(Named::End) => fix(QuickFixMessage::Last),
        _ => None,
    }
}

/// What Escape sends while the question to begin again is up.
pub fn hermes_card_close_message(facts: &KeyFacts) -> Option<Message> {
    facts.start_over_dialog.then_some(Message::HermesOverview(
        HermesOverviewMessage::CancelStartOver,
    ))
}
