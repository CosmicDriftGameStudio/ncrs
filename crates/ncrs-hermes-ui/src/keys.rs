//! What a key does on the Hermes pages. The app decides who a key belongs to
//! and calls these in its own order; what the keymap binds, it tells through
//! [`Bound`].

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use ncrs_index::QuerySpec;
use ncrs_pipeline::Load;

use crate::filing_pieces::TargetEdit;
use crate::message::{
    FilingStatusMessage, HermesFocus, HermesOverviewMessage, HermesSetupMessage, Message,
    QuickFixMessage, ReorganisationPlanMessage, ReviewListMessage, RuleFormMessage,
    RuleSuggestionsMessage, RulesMessage, SavedQueriesMessage, TagsMessage,
};

/// What the keys read of the state of the app.
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyFacts {
    /// The arrows are the sidebar's.
    pub hermes_sidebar: bool,
    pub review_list_editing: bool,
    pub review_list_replanning: bool,
    pub rules_form: bool,
    pub rules_confirming_delete: bool,
    pub hermes_overview_editing: bool,
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
    Focus(HermesFocus),
    /// Switches to the page this many places on in the sidebar.
    Step(isize),
    Quit,
}

fn page(message: Message) -> Option<PageKey> {
    Some(PageKey::Message(message))
}

/// A key the page does not know: it still quits when the keymap says so.
fn quit_if_bound(press: &KeyPress<'_>, bound: &dyn Fn() -> Bound) -> Option<PageKey> {
    (bound() == Bound::Quit && press.not_typed_by_text_field).then_some(PageKey::Quit)
}

/// The arrows between the sidebar and the page of a Hermes card: Left leaves
/// the page for the sidebar, where Up and Down switch the page and Right or
/// Enter go back. A form that is up keeps the arrows for its text fields.
pub fn hermes_focus_key(facts: &KeyFacts, press: &KeyPress<'_>, form_up: bool) -> Option<PageKey> {
    if !press.modifiers.is_empty() || form_up {
        return None;
    }
    match (press.key.as_ref(), facts.hermes_sidebar) {
        (Key::Named(Named::ArrowLeft), false) => Some(PageKey::Focus(HermesFocus::Sidebar)),
        (Key::Named(Named::ArrowUp), true) => Some(PageKey::Step(-1)),
        (Key::Named(Named::ArrowDown), true) => Some(PageKey::Step(1)),
        (Key::Named(Named::ArrowRight | Named::Enter), true) => {
            Some(PageKey::Focus(HermesFocus::Content))
        }
        _ => None,
    }
}

/// The plan is a page: Enter runs the selection, Space selects or deselects
/// what the cursor is on, the arrows move.
pub fn reorganisation_plan_key(
    facts: &KeyFacts,
    press: &KeyPress<'_>,
    bound: &dyn Fn() -> Bound,
) -> Option<PageKey> {
    let plan = |message| page(Message::ReorganisationPlan(message));
    let plain = press.modifiers.is_empty();
    if let Some(focus) = hermes_focus_key(facts, press, false) {
        return Some(focus);
    }
    match press.key.as_ref() {
        Key::Named(Named::Escape) => plan(ReorganisationPlanMessage::Close),
        Key::Named(Named::Enter) => plan(ReorganisationPlanMessage::Execute),
        Key::Named(Named::Space) if plain => plan(ReorganisationPlanMessage::ToggleCursor),
        Key::Named(Named::ArrowUp) if plain => plan(ReorganisationPlanMessage::Move(-1)),
        Key::Named(Named::ArrowDown) if plain => plan(ReorganisationPlanMessage::Move(1)),
        Key::Character("r") if plain => plan(ReorganisationPlanMessage::Replan),
        _ => quit_if_bound(press, bound),
    }
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

/// The review list is a card: Enter confirms, the arrows move in the list, `e`
/// opens the form. With the form up the fields take the typing; Escape is
/// `InputState`'s then.
pub fn review_list_key(
    facts: &KeyFacts,
    press: &KeyPress<'_>,
    bound: &dyn Fn() -> Bound,
) -> Option<PageKey> {
    let review = |message| page(Message::ReviewList(message));
    let plain = press.modifiers.is_empty();
    if let Some(focus) = hermes_focus_key(facts, press, facts.review_list_editing) {
        return Some(focus);
    }
    match press.key.as_ref() {
        Key::Named(Named::Escape) => review(ReviewListMessage::Close),
        Key::Named(Named::Enter) if facts.review_list_replanning => {
            review(ReviewListMessage::ReplanConfirm)
        }
        Key::Named(Named::Enter) => review(ReviewListMessage::Confirm),
        Key::Named(Named::F3) if plain => review(ReviewListMessage::TogglePreview),
        Key::Named(Named::Tab) if facts.review_list_editing => {
            review(ReviewListMessage::FocusField {
                backwards: press.modifiers.shift(),
            })
        }
        Key::Named(Named::Tab) if !facts.hermes_sidebar => review(ReviewListMessage::Edit),
        Key::Named(Named::ArrowUp) if plain && !facts.review_list_editing => {
            review(ReviewListMessage::Move(-1))
        }
        Key::Named(Named::ArrowDown) if plain && !facts.review_list_editing => {
            review(ReviewListMessage::Move(1))
        }
        _ => quit_if_bound(press, bound),
    }
}

/// The reports page: the arrows move in the list, Enter answers the step that
/// is up or else saves a changed name or starts the export, `n` opens a new
/// search and Delete asks to delete the report. The name field takes the keys
/// it types.
pub fn saved_queries_key(
    facts: &KeyFacts,
    press: &KeyPress<'_>,
    bound: &dyn Fn() -> Bound,
) -> Option<PageKey> {
    let queries = |message| page(Message::SavedQueries(message));
    let plain = press.modifiers.is_empty();
    if let Some(focus) = hermes_focus_key(facts, press, false) {
        return Some(focus);
    }
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
    facts: &KeyFacts,
    press: &KeyPress<'_>,
    bound: &dyn Fn() -> Bound,
) -> Option<PageKey> {
    let tags = |message| page(Message::Tags(message));
    let plain = press.modifiers.is_empty();
    if let Some(focus) = hermes_focus_key(facts, press, false) {
        return Some(focus);
    }
    match press.key.as_ref() {
        Key::Named(Named::Escape) => tags(TagsMessage::Close),
        Key::Named(Named::Enter) if plain => tags(TagsMessage::ConfirmDelete),
        Key::Named(Named::Space) if plain => tags(TagsMessage::ToggleSelected),
        Key::Named(Named::ArrowUp) if plain => tags(TagsMessage::Move(-1)),
        Key::Named(Named::ArrowDown) if plain => tags(TagsMessage::Move(1)),
        _ => quit_if_bound(press, bound),
    }
}

/// The view is a card: Enter confirms, the arrows move in the list and the
/// letters decide (`e`dit, `d`iscard, `r`e-analyse).
pub fn rule_suggestions_key(
    facts: &KeyFacts,
    press: &KeyPress<'_>,
    bound: &dyn Fn() -> Bound,
) -> Option<PageKey> {
    let suggestions = |message| page(Message::RuleSuggestions(message));
    let plain = press.modifiers.is_empty();
    if let Some(focus) = hermes_focus_key(facts, press, false) {
        return Some(focus);
    }
    match press.key.as_ref() {
        Key::Named(Named::Escape) => suggestions(RuleSuggestionsMessage::Close),
        Key::Named(Named::Enter) => suggestions(RuleSuggestionsMessage::Confirm),
        Key::Named(Named::ArrowUp) if plain => suggestions(RuleSuggestionsMessage::Move(-1)),
        Key::Named(Named::ArrowDown) if plain => suggestions(RuleSuggestionsMessage::Move(1)),
        Key::Character("e") if plain => suggestions(RuleSuggestionsMessage::Edit),
        Key::Character("d") if plain => suggestions(RuleSuggestionsMessage::Discard),
        Key::Character("r") if plain => suggestions(RuleSuggestionsMessage::Reanalyse),
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
    if let Some(focus) = hermes_focus_key(facts, press, facts.rules_form) {
        return Some(focus);
    }
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

/// The overview has one text field, for the name of a folder of the default
/// target, and it exists only while it is open. While it has the focus the
/// letters, the digits and the arrows are its, Enter adds the folder and
/// Escape closes the field; otherwise the page has the keys of the pause, the
/// review list and the sidebar.
pub fn hermes_overview_key(
    facts: &KeyFacts,
    press: &KeyPress<'_>,
    bound: &dyn Fn() -> Bound,
) -> Option<PageKey> {
    let overview = |message| page(Message::HermesOverview(message));
    let editing = facts.hermes_overview_editing;
    let plain = press.modifiers.is_empty();
    if let Some(focus) = hermes_focus_key(facts, press, editing) {
        return Some(focus);
    }
    match press.key.as_ref() {
        Key::Named(Named::Escape) if editing => overview(HermesOverviewMessage::TargetEdited(
            TargetEdit::OwnNameCancel,
        )),
        Key::Named(Named::Escape) => overview(HermesOverviewMessage::Close),
        Key::Named(Named::Enter) if editing => overview(HermesOverviewMessage::TargetEdited(
            TargetEdit::OwnNameSubmit,
        )),
        Key::Character("p") if plain && !editing => {
            page(Message::FilingStatus(FilingStatusMessage::TogglePause))
        }
        Key::Named(Named::Enter) if plain => overview(HermesOverviewMessage::ShowReviews),
        _ if !press.not_typed_by_text_field => None,
        _ => match bound() {
            Bound::StatusToggle => page(Message::FilingStatus(FilingStatusMessage::Toggle)),
            Bound::Quit => Some(PageKey::Quit),
            Bound::Other => None,
        },
    }
}

pub fn filing_status_key(
    facts: &KeyFacts,
    press: &KeyPress<'_>,
    bound: &dyn Fn() -> Bound,
) -> Option<PageKey> {
    use FilingStatusMessage as Page;
    let status = |message| page(Message::FilingStatus(message));
    let plain = press.modifiers.is_empty();
    if let Some(focus) = hermes_focus_key(facts, press, false) {
        return Some(focus);
    }
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
