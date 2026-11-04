//! What a key does on the Hermes pages. The app decides who a key belongs to
//! and calls these in its own order; what the keymap binds, it tells through
//! [`Bound`].

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use ncrs_index::QuerySpec;
use ncrs_pipeline::Load;

use crate::message::{
    FilingStatusMessage, HermesOverviewMessage, HermesSetupMessage, Message, QuickFixMessage,
    RuleFormMessage, RulesMessage, SavedQueriesMessage, TagsMessage,
};

/// What the keys read of the state of the app.
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyFacts {
    pub rules_form: bool,
    pub rules_confirming_delete: bool,
    pub start_over_dialog: bool,
    pub hermes_setup: bool,
}

/// The key that is pressed.
#[derive(Debug, Clone, Copy)]
pub struct KeyPress<'a> {
    pub key: &'a Key,
    pub modifiers: Modifiers,
    /// Nothing a text field would type: a function key, or a chord with Super
    /// or Ctrl.
    pub not_typed_by_text_field: bool,
}

/// What the keymap binds the key to, as far as the pages care.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    Quit,
    /// The key that opens and closes the status page.
    StatusToggle,
    Other,
}

/// What a key means on a page. The app turns it into its own message.
#[derive(Debug, Clone, PartialEq)]
pub enum PageKey {
    Message(Message),
    Quit,
}

fn page(message: Message) -> Option<PageKey> {
    Some(PageKey::Message(message))
}

/// A key the page does not know: it still quits when the keymap says so.
fn quit_if_bound(press: &KeyPress<'_>, bound: &dyn Fn() -> Bound) -> Option<PageKey> {
    (bound() == Bound::Quit && press.not_typed_by_text_field).then_some(PageKey::Quit)
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

/// The reports page: the arrows move in the list, Enter answers the step that
/// is up or else saves a changed name or starts the export, `n` opens a new
/// search and Delete asks to delete the report. The name field takes the keys
/// it types.
pub fn saved_queries_key(
    _facts: &KeyFacts,
    press: &KeyPress<'_>,
    bound: &dyn Fn() -> Bound,
) -> Option<PageKey> {
    let queries = |message| page(Message::SavedQueries(message));
    let plain = press.modifiers.is_empty();
    match press.key.as_ref() {
        Key::Named(Named::Escape) => queries(SavedQueriesMessage::Close),
        Key::Named(Named::Enter) if plain => queries(SavedQueriesMessage::Confirm),
        Key::Character("n") if plain => {
            queries(SavedQueriesMessage::NewSearch(QuerySpec::default()))
        }
        Key::Named(Named::Delete) if plain => queries(SavedQueriesMessage::AskDelete),
        Key::Named(Named::ArrowUp) if plain => queries(SavedQueriesMessage::Move(-1)),
        Key::Named(Named::ArrowDown) if plain => queries(SavedQueriesMessage::Move(1)),
        _ => quit_if_bound(press, bound),
    }
}

/// The tags page: the arrows move in the table, Space switches the tag or
/// group under the cursor, Enter answers the delete question, Delete asks it.
/// The search and the name field take the keys they type.
pub fn tags_key(
    _facts: &KeyFacts,
    press: &KeyPress<'_>,
    bound: &dyn Fn() -> Bound,
) -> Option<PageKey> {
    let tags = |message| page(Message::Tags(message));
    let plain = press.modifiers.is_empty();
    match press.key.as_ref() {
        Key::Named(Named::Escape) => tags(TagsMessage::Close),
        Key::Named(Named::Enter) if plain => tags(TagsMessage::ConfirmDelete),
        Key::Named(Named::Space) if plain => tags(TagsMessage::ToggleSelected),
        Key::Named(Named::ArrowUp) if plain => tags(TagsMessage::Move(-1)),
        Key::Named(Named::ArrowDown) if plain => tags(TagsMessage::Move(1)),
        _ => quit_if_bound(press, bound),
    }
}

/// The rules page: the arrows move in the list, Alt with the arrows moves the
/// rule, Space switches it, Delete asks to delete it, Enter answers that
/// question or else puts the keys into the form of the rule, and `n` starts a
/// new one. With the keys in the form the fields take them; Escape and
/// Command+Enter are the page's.
pub fn rules_key(
    facts: &KeyFacts,
    press: &KeyPress<'_>,
    bound: &dyn Fn() -> Bound,
) -> Option<PageKey> {
    let rules = |message| page(Message::Rules(message));
    let plain = press.modifiers.is_empty();
    let modifiers = press.modifiers;
    if facts.rules_form {
        return match press.key.as_ref() {
            Key::Named(Named::Escape) => rules(RulesMessage::Close),
            Key::Named(Named::Enter) if modifiers == Modifiers::COMMAND => {
                rules(RulesMessage::Form(RuleFormMessage::Save))
            }
            _ => quit_if_bound(press, bound),
        };
    }
    match press.key.as_ref() {
        Key::Named(Named::Escape) => rules(RulesMessage::Close),
        Key::Named(Named::Enter) if plain && facts.rules_confirming_delete => {
            rules(RulesMessage::ConfirmDelete)
        }
        Key::Named(Named::Enter) if modifiers == Modifiers::COMMAND => {
            rules(RulesMessage::Form(RuleFormMessage::Save))
        }
        Key::Named(Named::Enter | Named::Tab) if plain => rules(RulesMessage::Edit),
        Key::Character("n") if plain => rules(RulesMessage::New),
        Key::Named(Named::Space) if plain => rules(RulesMessage::Toggle),
        Key::Named(Named::Delete) if plain => rules(RulesMessage::AskDelete),
        Key::Named(Named::ArrowUp) if plain => rules(RulesMessage::Move(-1)),
        Key::Named(Named::ArrowDown) if plain => rules(RulesMessage::Move(1)),
        Key::Named(Named::ArrowUp) if modifiers == Modifiers::ALT => {
            rules(RulesMessage::Reorder(-1))
        }
        Key::Named(Named::ArrowDown) if modifiers == Modifiers::ALT => {
            rules(RulesMessage::Reorder(1))
        }
        _ => quit_if_bound(press, bound),
    }
}

/// What Escape sends while the question to begin again or the setup card is up.
pub fn hermes_card_close_message(facts: &KeyFacts) -> Option<Message> {
    if facts.start_over_dialog {
        Some(Message::HermesOverview(
            HermesOverviewMessage::CancelStartOver,
        ))
    } else if facts.hermes_setup {
        Some(Message::HermesSetup(HermesSetupMessage::Later))
    } else {
        None
    }
}

pub fn filing_status_key(
    _facts: &KeyFacts,
    press: &KeyPress<'_>,
    bound: &dyn Fn() -> Bound,
) -> Option<PageKey> {
    use FilingStatusMessage as Page;
    let status = |message| page(Message::FilingStatus(message));
    let plain = press.modifiers.is_empty();
    match press.key.as_ref() {
        Key::Named(Named::Escape) => status(Page::Close),
        Key::Named(Named::ArrowUp) if plain => status(Page::MoveError(-1)),
        Key::Named(Named::ArrowDown) if plain => status(Page::MoveError(1)),
        Key::Named(Named::Enter) if plain => status(Page::ProcessAgain),
        Key::Named(Named::Space) if plain => status(Page::ToggleSelectedErrorChecked),
        Key::Character("p") if plain => status(Page::TogglePause),
        Key::Character("1") if plain => page(Message::HermesOverview(
            HermesOverviewMessage::SetLoad(Load::Gentle),
        )),
        Key::Character("2") if plain => page(Message::HermesOverview(
            HermesOverviewMessage::SetLoad(Load::Balanced),
        )),
        Key::Character("3") if plain => page(Message::HermesOverview(
            HermesOverviewMessage::SetLoad(Load::Full),
        )),
        Key::Character("+") if plain => status(Page::StepWorkers(1)),
        Key::Character("-") if plain => status(Page::StepWorkers(-1)),
        _ => match bound() {
            Bound::StatusToggle => status(Page::Close),
            Bound::Quit => Some(PageKey::Quit),
            Bound::Other => None,
        },
    }
}
