//! What a key does in the small Hermes cards. The pages of Hermes have no keys
//! of their own: they are pages of the settings and use its keys.

use iced::keyboard::key::Named;
use iced::keyboard::Key;

use crate::message::{HermesFileMessage, HermesOverviewMessage, Message, QuickFixMessage};

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

/// The question about many files to process again: Enter says yes, Escape no;
/// any other key does nothing until it is answered.
pub fn confirm_again_key(key: &Key) -> Option<Message> {
    let file = |message| Some(Message::HermesFile(message));
    match key.as_ref() {
        Key::Named(Named::Escape) => file(HermesFileMessage::CancelProcessAgain),
        Key::Named(Named::Enter) => file(HermesFileMessage::ConfirmProcessAgain),
        _ => None,
    }
}

/// What Escape sends while the question to begin again is up.
pub fn hermes_card_close_message(facts: &KeyFacts) -> Option<Message> {
    facts.start_over_dialog.then_some(Message::HermesOverview(
        HermesOverviewMessage::CancelStartOver,
    ))
}

/// The source dialog: Escape closes it, Enter saves (on "+ Ziel…" it opens the
/// dialog of a target), Tab and Shift+Tab walk the fields, the arrows change the choice that is focused and Space presses
/// what is focused. Typing goes to the text field that has the focus and never reaches here.
pub fn source_dialog_key(key: &Key, shift: bool) -> Option<Message> {
    use crate::message::SortingMessage;
    let sorting = |message| Some(Message::Sorting(message));
    match key.as_ref() {
        Key::Named(Named::Escape) => sorting(SortingMessage::Cancel),
        Key::Named(Named::Enter) => sorting(SortingMessage::Enter),
        Key::Named(Named::Tab) if shift => sorting(SortingMessage::FocusPrevious),
        Key::Named(Named::Tab) => sorting(SortingMessage::FocusNext),
        Key::Named(
            Named::ArrowLeft | Named::ArrowRight | Named::ArrowUp | Named::ArrowDown,
        ) => sorting(SortingMessage::StepChoice),
        Key::Named(Named::Space) => sorting(SortingMessage::Press),
        _ => None,
    }
}

/// The dialog of a target over the source dialog.
pub fn target_dialog_key(key: &Key) -> Option<Message> {
    use crate::message::TargetsMessage;
    match key.as_ref() {
        Key::Named(Named::Escape) => Some(Message::Targets(TargetsMessage::Cancel)),
        Key::Named(Named::Enter) => Some(Message::Targets(TargetsMessage::Save)),
        _ => None,
    }
}

/// The question before a target is removed.
pub fn target_removal_key(key: &Key) -> Option<Message> {
    use crate::message::TargetsMessage;
    match key.as_ref() {
        Key::Named(Named::Escape) => Some(Message::Targets(TargetsMessage::CancelRemove)),
        Key::Named(Named::Enter) => Some(Message::Targets(TargetsMessage::ConfirmRemove)),
        _ => None,
    }
}

/// The rules Hermes proposes: the arrows move, Space ticks, Enter takes the
/// ticked ones over.
pub fn suggest_dialog_key(key: &Key) -> Option<Message> {
    use crate::message::SuggestMessage;
    let message = match key.as_ref() {
        Key::Named(Named::ArrowUp) => SuggestMessage::CursorUp,
        Key::Named(Named::ArrowDown) => SuggestMessage::CursorDown,
        Key::Named(Named::Space) => SuggestMessage::ToggleCursor,
        Key::Named(Named::Enter) => SuggestMessage::Adopt,
        Key::Named(Named::Escape) => SuggestMessage::Close,
        _ => return None,
    };
    Some(Message::Suggest(message))
}
