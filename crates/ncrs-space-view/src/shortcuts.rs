//! The actions of the Platz tab with the keys they ship with; [`SpaceKeys`]
//! says which key does what once `[keys.space]` is applied.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use ncrs_i18n::{Language, Msg};

use crate::decide::DecideMessage;
use crate::keys::{Chord, SpaceKeys};
use crate::message::Message;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    DeleteSafe,
    RemoveWorktrees,
    Rescan,
    Retry,
    AskHermes,
    ShowInPanel,
    DeleteOnlyThis,
    Filter,
    Trash,
    UndoTrash,
    EmptyTrash,
}

impl Shortcut {
    pub const ALL: &'static [Self] = &[
        Self::DeleteSafe,
        Self::RemoveWorktrees,
        Self::Rescan,
        Self::Retry,
        Self::AskHermes,
        Self::ShowInPanel,
        Self::DeleteOnlyThis,
        Self::Filter,
        Self::Trash,
        Self::UndoTrash,
        Self::EmptyTrash,
    ];

    /// The name of the action in `[keys.space]`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::DeleteSafe => "delete_safe",
            Self::RemoveWorktrees => "remove_worktrees",
            Self::Rescan => "rescan",
            Self::Retry => "retry",
            Self::AskHermes => "ask_hermes",
            Self::ShowInPanel => "show_in_panel",
            Self::DeleteOnlyThis => "delete_only_this",
            Self::Filter => "filter",
            Self::Trash => "trash",
            Self::UndoTrash => "undo_trash",
            Self::EmptyTrash => "empty_trash",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|shortcut| shortcut.name() == name)
    }

    /// The place in [`Self::ALL`], which is where [`SpaceKeys`] keeps the keys.
    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|shortcut| *shortcut == self)
            .unwrap_or_default()
    }

    /// The default key is one the whole app already gives the same meaning, or one that
    /// only counts in this tab: the check against the app keymap skips these.
    pub const fn shares_an_app_key(self) -> bool {
        matches!(
            self,
            Self::AskHermes | Self::DeleteOnlyThis | Self::UndoTrash
        )
    }

    /// The key it ships with; `command` is what Cmd or Ctrl is on the platform.
    pub fn default_chord(self, command: Modifiers) -> Chord {
        let shifted = command | Modifiers::SHIFT;
        let letter = |held: Modifiers, letter: &str| (held, Key::Character(letter.into()));
        let named = |held: Modifiers, named: Named| (held, Key::Named(named));
        match self {
            Self::DeleteSafe => named(command, Named::Enter),
            Self::RemoveWorktrees => named(shifted, Named::Enter),
            Self::Rescan => letter(command, "r"),
            Self::Retry => letter(shifted, "r"),
            Self::AskHermes => named(Modifiers::empty(), Named::F2),
            Self::ShowInPanel => letter(command | Modifiers::ALT, "f"),
            Self::DeleteOnlyThis => named(Modifiers::empty(), Named::F8),
            Self::Filter => letter(Modifiers::empty(), "/"),
            Self::Trash => named(command, Named::Backspace),
            Self::UndoTrash => letter(command, "z"),
            Self::EmptyTrash => letter(command, "e"),
        }
    }

    pub fn label(self, keys: &SpaceKeys) -> String {
        keys.label(self)
    }

    /// The name in the command list.
    pub const fn command_label(self) -> Msg {
        match self {
            Self::DeleteSafe => Msg::SpaceCommandDeleteSafe,
            Self::RemoveWorktrees => Msg::SpaceCommandRemoveWorktrees,
            Self::Rescan => Msg::SpaceCommandRescan,
            Self::Retry => Msg::SpaceCommandRetry,
            Self::AskHermes => Msg::SpaceCommandAsk,
            Self::ShowInPanel => Msg::SpaceCommandShowInPanel,
            Self::DeleteOnlyThis => Msg::SpaceCommandDeleteOne,
            Self::Filter => Msg::SpaceCommandFilter,
            Self::Trash => Msg::SpaceCommandTrash,
            Self::UndoTrash => Msg::SpaceDecideSelectionUndo,
            Self::EmptyTrash => Msg::SpaceCommandEmptyTrash,
        }
    }

    pub fn message(self) -> Message {
        match self {
            Self::DeleteSafe => Message::DeleteSafe,
            Self::RemoveWorktrees => Message::RemoveWorktrees,
            Self::Rescan => Message::Rescan,
            Self::Retry => Message::Retry,
            Self::AskHermes => Message::AskHermes,
            Self::ShowInPanel => Message::ShowInPanelAtCursor,
            Self::DeleteOnlyThis => Message::DeleteOnlyThis,
            Self::Filter => Message::FocusFilter,
            Self::Trash => Message::Decide(DecideMessage::Trash),
            Self::UndoTrash => Message::Decide(DecideMessage::Undo),
            Self::EmptyTrash => Message::Decide(DecideMessage::ToggleEmptyTrash),
        }
    }
}

/// Every action the command list offers while the Platz tab is shown, with its
/// label and key. The place in the list is what the app sends back to [`message_of_command`].
pub fn commands(lang: Language, keys: &SpaceKeys) -> Vec<(String, String)> {
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
