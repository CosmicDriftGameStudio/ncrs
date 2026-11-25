//! What a key does while the Git tab is up.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

use ncrs_ui_kit::splitter::{Direction, Step};

use crate::git_keys::GitKeys;
use crate::message::Message;
use crate::panes::Split;
use crate::shortcuts::Shortcut;
use crate::state::Area;

/// What the tab is doing, as far as the keys care.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct KeyContext {
    pub filter_focused: bool,
    pub in_diff: bool,
    pub dialog_open: bool,
    pub picker_open: bool,
    pub area: Area,
}

/// The message of a press. Escape is not here: the app sends
/// [`Message::Escape`] whatever field took the key. While the filter has the
/// keyboard only the arrows that leave it count.
pub fn git_view_key(
    keys: &GitKeys,
    key: &Key,
    modifiers: Modifiers,
    context: KeyContext,
) -> Option<Message> {
    if context.dialog_open {
        return (modifiers.is_empty() && key.as_ref() == Key::Named(Named::Enter))
            .then_some(Message::Enter);
    }
    if context.picker_open {
        return picker_key(keys, key, modifiers);
    }
    if context.filter_focused {
        return None;
    }
    if let Some(shortcut) = Shortcut::pressed(keys, key, modifiers) {
        if shortcut == Shortcut::ToggleStage && context.area == Area::Commits && !context.in_diff {
            return Some(Message::MarkCommit);
        }
        let applies = match shortcut {
            Shortcut::PreviousFile | Shortcut::NextFile => {
                context.in_diff || context.area == Area::Detail
            }
            Shortcut::ToggleStage => context.area == Area::Detail,
            Shortcut::CherryPick => context.area == Area::Commits,
            _ => true,
        };
        return applies.then(|| shortcut.message());
    }
    // "/" is a shifted key on some layouts.
    let slash_modifiers = modifiers.is_empty() || modifiers == Modifiers::SHIFT;
    if slash_modifiers && key.as_ref() == Key::Character("/") {
        return Some(Message::FocusFilter);
    }
    if modifiers == Modifiers::SHIFT && key.as_ref() == Key::Named(Named::Tab) {
        return Some(Message::PreviousArea);
    }
    if let Some(split) = context.area.split() {
        return splitter_key(split, key, modifiers);
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
        Key::Named(Named::ContextMenu) => Some(Message::OpenMenuKey),
        Key::Named(Named::F3) if context.in_diff => Some(Message::ViewFile),
        Key::Character("n") if context.in_diff => Some(Message::NextHunk),
        Key::Character("p") if context.in_diff => Some(Message::PreviousHunk),
        _ => None,
    }
}

/// An open picker keeps the arrows, Enter and its own two keys; the search
/// field gets the rest.
fn picker_key(keys: &GitKeys, key: &Key, modifiers: Modifiers) -> Option<Message> {
    if modifiers == Modifiers::COMMAND && key.as_ref() == Key::Named(Named::Enter) {
        return Some(Message::PickerEnterWithStash);
    }
    if Shortcut::pressed(keys, key, modifiers) == Some(Shortcut::NewBranch) {
        return Some(Message::PickerNewBranch);
    }
    if !modifiers.is_empty() {
        return None;
    }
    match key.as_ref() {
        Key::Named(Named::ArrowUp) => Some(Message::Up),
        Key::Named(Named::ArrowDown) => Some(Message::Down),
        Key::Named(Named::Enter) => Some(Message::PickerSubmit),
        _ => None,
    }
}

/// The arrows on a focused splitter move the bar: 16 px, with ⇧ 64 px; Home resets.
fn splitter_key(split: Split, key: &Key, modifiers: Modifiers) -> Option<Message> {
    let step = if modifiers == Modifiers::SHIFT {
        Step::Coarse
    } else if modifiers.is_empty() {
        Step::Fine
    } else {
        return None;
    };
    let pane_direction = |rightwards: bool| {
        if rightwards == split.grows_rightwards() {
            Direction::Wider
        } else {
            Direction::Narrower
        }
    };
    match key.as_ref() {
        Key::Named(Named::ArrowRight) => {
            Some(Message::SplitNudge(split, step, pane_direction(true)))
        }
        Key::Named(Named::ArrowLeft) => {
            Some(Message::SplitNudge(split, step, pane_direction(false)))
        }
        Key::Named(Named::Home) if modifiers.is_empty() => Some(Message::SplitReset(split)),
        Key::Named(Named::Tab) if modifiers.is_empty() => Some(Message::NextArea),
        _ => None,
    }
}

/// What a press means while a field of the commit box has the keyboard: the
/// commit keys and the jump to the title, and Tab, which would type a tab.
pub fn commit_field_key(keys: &GitKeys, key: &Key, modifiers: Modifiers) -> Option<Message> {
    if key.as_ref() == Key::Named(Named::Tab) {
        return match modifiers {
            m if m.is_empty() => Some(Message::NextArea),
            m if m == Modifiers::SHIFT => Some(Message::PreviousArea),
            _ => None,
        };
    }
    match Shortcut::pressed(keys, key, modifiers)? {
        shortcut @ (Shortcut::Commit | Shortcut::CommitAndPush | Shortcut::FocusCommitTitle) => {
            Some(shortcut.message())
        }
        _ => None,
    }
}
