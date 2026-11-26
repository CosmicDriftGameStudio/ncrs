//! The actions of the Repos tab with the keys they ship with; [`ReposKeys`]
//! says which key does what once `[keys.repos]` is applied.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use ncrs_i18n::{Language, Msg};

use crate::bulk::BulkAction;
use crate::cleanup::CleanupMessage;
use crate::keys::{Chord, ReposKeys};
use crate::message::Message;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    Fetch,
    SwitchToDefault,
    PullFastForward,
    Both,
    Push,
    RetryFailed,
    OpenInTabs,
    CleanUp,
    UndoCleanup,
    SelectAll,
    Favourite,
    Remove,
    ShowInFiles,
    ShowWorktrees,
}

impl Shortcut {
    pub const ALL: &'static [Self] = &[
        Self::Fetch,
        Self::SwitchToDefault,
        Self::PullFastForward,
        Self::Both,
        Self::Push,
        Self::RetryFailed,
        Self::OpenInTabs,
        Self::CleanUp,
        Self::UndoCleanup,
        Self::SelectAll,
        Self::Favourite,
        Self::Remove,
        Self::ShowInFiles,
        Self::ShowWorktrees,
    ];

    /// The name of the action in `[keys.repos]`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Fetch => "fetch",
            Self::SwitchToDefault => "switch_to_default",
            Self::PullFastForward => "pull",
            Self::Both => "both",
            Self::Push => "push",
            Self::RetryFailed => "retry_failed",
            Self::OpenInTabs => "open_in_tabs",
            Self::CleanUp => "clean_up",
            Self::UndoCleanup => "undo_cleanup",
            Self::SelectAll => "select_all",
            Self::Favourite => "favourite",
            Self::Remove => "remove",
            Self::ShowInFiles => "show_in_files",
            Self::ShowWorktrees => "show_worktrees",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|shortcut| shortcut.name() == name)
    }

    /// The place in [`Self::ALL`], which is where [`ReposKeys`] keeps the keys.
    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|shortcut| *shortcut == self)
            .unwrap_or_default()
    }

    /// The default key is one the whole app already gives the same meaning,
    /// or one that only counts in a file panel: the check against the app
    /// keymap skips these.
    pub const fn shares_an_app_key(self) -> bool {
        matches!(self, Self::SelectAll | Self::Favourite | Self::Remove)
    }

    /// The key it ships with; `command` is what Cmd or Ctrl is on the
    /// platform. The keys of the Git tab (⇧⌘E, ⇧⌘L, ⇧⌘O) mean the same here.
    pub fn default_chord(self, command: Modifiers) -> Chord {
        let shifted = command | Modifiers::SHIFT;
        let letter = |held: Modifiers, letter: &str| (held, Key::Character(letter.into()));
        let named = |held: Modifiers, named: Named| (held, Key::Named(named));
        match self {
            Self::Fetch => letter(shifted, "e"),
            Self::SwitchToDefault => letter(shifted, "o"),
            Self::PullFastForward => letter(shifted, "l"),
            Self::Both => letter(shifted, "j"),
            Self::Push => letter(shifted, "u"),
            Self::RetryFailed => letter(command, "r"),
            Self::OpenInTabs => named(shifted, Named::Enter),
            Self::CleanUp => letter(command | Modifiers::ALT, "a"),
            Self::UndoCleanup => letter(shifted, "y"),
            Self::SelectAll => letter(command, "a"),
            Self::Favourite => letter(Modifiers::empty(), "*"),
            Self::Remove => named(Modifiers::empty(), Named::Delete),
            Self::ShowInFiles => letter(command | Modifiers::ALT, "f"),
            Self::ShowWorktrees => letter(command | Modifiers::ALT, "w"),
        }
    }

    pub fn label(self, keys: &ReposKeys) -> String {
        keys.label(self)
    }

    /// The name in the command list and in the menu.
    pub const fn command_label(self) -> Msg {
        match self {
            Self::Fetch => Msg::GitViewToolbarFetch,
            Self::SwitchToDefault => Msg::ReposActionSwitchDefault,
            Self::PullFastForward => Msg::ReposActionPullFf,
            Self::Both => Msg::ReposActionBoth,
            Self::Push => Msg::GitViewToolbarPush,
            Self::RetryFailed => Msg::ReposRetryFailed,
            Self::OpenInTabs => Msg::ReposOpenInTabs,
            Self::CleanUp => Msg::ReposCommandCleanup,
            Self::UndoCleanup => Msg::ReposCommandUndoCleanup,
            Self::SelectAll => Msg::ReposSelectAll,
            Self::Favourite => Msg::ReposToggleFavourite,
            Self::Remove => Msg::ReposRemove,
            Self::ShowInFiles => Msg::ReposShowInFiles,
            Self::ShowWorktrees => Msg::ReposShowWorktrees,
        }
    }

    pub fn message(self) -> Message {
        match self {
            Self::Fetch => Message::Bulk(BulkAction::Fetch),
            Self::SwitchToDefault => Message::Bulk(BulkAction::SwitchToDefault),
            Self::PullFastForward => Message::Bulk(BulkAction::PullFastForward),
            Self::Both => Message::Bulk(BulkAction::Both),
            Self::Push => Message::Bulk(BulkAction::Push),
            Self::RetryFailed => Message::RetryFailed,
            Self::OpenInTabs => Message::OpenInTabs,
            Self::CleanUp => Message::Cleanup(CleanupMessage::Open),
            Self::UndoCleanup => Message::Cleanup(CleanupMessage::Undo),
            Self::SelectAll => Message::MarkAll,
            Self::Favourite => Message::ToggleFavouriteAtCursor,
            Self::Remove => Message::RemoveAtCursor,
            Self::ShowInFiles => Message::ShowInFilesAtCursor,
            Self::ShowWorktrees => Message::ShowWorktreesToggled,
        }
    }
}

/// Every action the command list offers while the Repos tab is shown, with
/// its label and key. The place in the list is what the app sends back to
/// [`message_of_command`].
pub fn commands(lang: Language, keys: &ReposKeys) -> Vec<(String, String)> {
    Shortcut::ALL
        .iter()
        .map(|shortcut| {
            let label = lang.text(shortcut.command_label()).to_owned();
            (label, shortcut.label(keys))
        })
        .collect()
}

/// The message of the command at `place` in [`commands`].
pub fn message_of_command(place: usize) -> Option<Message> {
    Shortcut::ALL.get(place).map(|shortcut| shortcut.message())
}
