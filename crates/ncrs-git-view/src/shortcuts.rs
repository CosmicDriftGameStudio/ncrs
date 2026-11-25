//! The Git actions with the keys they ship with; [`GitKeys`] says which key
//! does what once `[keys.git]` is applied. The key handler, the menus, the
//! tooltips and the status bar read that.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

use ncrs_i18n::{Language, Msg};

use crate::actions::{ConflictAction, MenuAction, Side};
use crate::git_keys::{Chord, GitKeys};
use crate::message::Message;
use crate::picker::PickerKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    Fetch,
    Pull,
    Push,
    Stash,
    Undo,
    Commit,
    StageAll,
    UnstageAll,
    DiscardAll,
    NewBranch,
    Checkout,
    Rename,
    Delete,
    Merge,
    Rebase,
    StashApply,
    StashPop,
    OpenMenu,
    TakeMine,
    TakeTheirs,
    TakeBoth,
    OpenEditor,
    MarkResolved,
    ResetConflict,
    RepoPicker,
    BranchPicker,
    JumpHead,
    JumpWip,
    CopySha,
    GoParent,
    ToggleTree,
}

impl Shortcut {
    pub const ALL: &'static [Self] = &[
        Self::Fetch,
        Self::Pull,
        Self::Push,
        Self::Stash,
        Self::Undo,
        Self::Commit,
        Self::StageAll,
        Self::UnstageAll,
        Self::DiscardAll,
        Self::NewBranch,
        Self::Checkout,
        Self::Rename,
        Self::Delete,
        Self::Merge,
        Self::Rebase,
        Self::StashApply,
        Self::StashPop,
        Self::OpenMenu,
        Self::TakeMine,
        Self::TakeTheirs,
        Self::TakeBoth,
        Self::OpenEditor,
        Self::MarkResolved,
        Self::ResetConflict,
        Self::RepoPicker,
        Self::BranchPicker,
        Self::JumpHead,
        Self::JumpWip,
        Self::CopySha,
        Self::GoParent,
        Self::ToggleTree,
    ];

    /// The name of the action in `[keys.git]`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Fetch => "fetch",
            Self::Pull => "pull",
            Self::Push => "push",
            Self::Stash => "stash",
            Self::Undo => "undo",
            Self::Commit => "commit",
            Self::StageAll => "stage_all",
            Self::UnstageAll => "unstage_all",
            Self::DiscardAll => "discard_all",
            Self::NewBranch => "new_branch",
            Self::Checkout => "checkout",
            Self::Rename => "rename",
            Self::Delete => "delete",
            Self::Merge => "merge",
            Self::Rebase => "rebase",
            Self::StashApply => "stash_apply",
            Self::StashPop => "stash_pop",
            Self::OpenMenu => "open_menu",
            Self::TakeMine => "take_mine",
            Self::TakeTheirs => "take_theirs",
            Self::TakeBoth => "take_both",
            Self::OpenEditor => "open_editor",
            Self::MarkResolved => "mark_resolved",
            Self::ResetConflict => "reset_conflict",
            Self::RepoPicker => "repo_picker",
            Self::BranchPicker => "branch_picker",
            Self::JumpHead => "jump_head",
            Self::JumpWip => "jump_wip",
            Self::CopySha => "copy_sha",
            Self::GoParent => "parent",
            Self::ToggleTree => "toggle_tree",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|shortcut| shortcut.name() == name)
    }

    /// The place in [`Self::ALL`], which is where [`GitKeys`] keeps the keys.
    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|shortcut| *shortcut == self)
            .unwrap_or_default()
    }

    /// The default key is one the whole app already gives the same meaning:
    /// edit, menu, rename. The check against the app keymap skips these.
    pub const fn shares_an_app_key(self) -> bool {
        matches!(
            self,
            Self::OpenEditor | Self::OpenMenu | Self::Rename | Self::CopySha | Self::GoParent
        )
    }

    /// The key it ships with; `command` is what Cmd or Ctrl is on the
    /// platform. ⇧⌘ plus a letter, the page keys of the app (⌘P, ⇧⌘P, ⇧⌘F,
    /// ⇧⌘B, ⇧⌘D, ⇧⌘H, ⌘K, ⌘,) stay free.
    pub fn default_chord(self, command: Modifiers) -> Chord {
        let shifted = command | Modifiers::SHIFT;
        let letter = |held: Modifiers, letter: &str| (held, Key::Character(letter.into()));
        let named = |held: Modifiers, named: Named| (held, Key::Named(named));
        match self {
            Self::Fetch => letter(shifted, "e"),
            Self::Pull => letter(shifted, "l"),
            Self::Push => letter(shifted, "u"),
            Self::Stash => letter(shifted, "s"),
            Self::Undo => letter(shifted, "y"),
            Self::Commit => named(command, Named::Enter),
            Self::StageAll => letter(shifted, "i"),
            Self::UnstageAll => letter(shifted, "r"),
            Self::DiscardAll => named(shifted, Named::Backspace),
            Self::NewBranch => letter(shifted, "t"),
            Self::Checkout => letter(shifted, "o"),
            Self::Rename => named(Modifiers::SHIFT, Named::F6),
            Self::Delete => named(command, Named::Backspace),
            Self::Merge => letter(shifted, "m"),
            Self::Rebase => letter(shifted, "x"),
            Self::StashApply => named(shifted, Named::Enter),
            Self::StashPop => letter(shifted, "w"),
            Self::OpenMenu => named(Modifiers::SHIFT, Named::F10),
            Self::TakeMine => named(shifted, Named::ArrowLeft),
            Self::TakeTheirs => named(shifted, Named::ArrowRight),
            Self::TakeBoth => named(shifted, Named::ArrowDown),
            Self::OpenEditor => named(Modifiers::empty(), Named::F4),
            Self::MarkResolved => letter(shifted, "j"),
            Self::ResetConflict => letter(shifted, "z"),
            Self::RepoPicker => letter(command, "o"),
            Self::BranchPicker => letter(command, "b"),
            Self::JumpHead => letter(Modifiers::empty(), "h"),
            Self::JumpWip => letter(Modifiers::empty(), "w"),
            Self::CopySha => letter(command, "c"),
            Self::GoParent => named(Modifiers::empty(), Named::Backspace),
            Self::ToggleTree => letter(Modifiers::empty(), "t"),
        }
    }

    /// The first key of the action, as the app-wide keymap spells it; none
    /// when `[keys.git]` took it away.
    pub fn binding(self, keys: &GitKeys) -> Option<Chord> {
        keys.chords(self).first().cloned()
    }

    /// The shortcut a press stands for.
    pub fn pressed(keys: &GitKeys, key: &Key, modifiers: Modifiers) -> Option<Self> {
        keys.pressed(key, modifiers)
    }

    /// The first key as this platform spells it: `⇧⌘E` on a Mac,
    /// `Ctrl+Shift+E` elsewhere; empty for an action without a key.
    pub fn label(self, keys: &GitKeys) -> String {
        keys.label(self)
    }

    /// The name in the command list; the menu key opens a menu and is no command.
    pub fn command_label(self) -> Option<Msg> {
        Some(match self {
            Self::Fetch => Msg::GitViewToolbarFetch,
            Self::Pull => Msg::GitViewToolbarPull,
            Self::Push => Msg::GitViewToolbarPush,
            Self::Stash => Msg::GitViewToolbarStash,
            Self::Undo => Msg::GitViewCommandUndo,
            Self::Commit => Msg::GitViewCommandCommit,
            Self::StageAll => Msg::GitViewStageAll,
            Self::UnstageAll => Msg::GitViewUnstageAll,
            Self::DiscardAll => Msg::GitViewDiscardAll,
            Self::NewBranch => Msg::GitViewMenuNewBranchFrom,
            Self::Checkout => Msg::GitViewMenuCheckout,
            Self::Rename => Msg::GitViewMenuRename,
            Self::Delete => Msg::GitViewMenuDelete,
            Self::Merge => Msg::GitViewCommandMerge,
            Self::Rebase => Msg::GitViewCommandRebase,
            Self::StashApply => Msg::GitViewCommandStashApply,
            Self::StashPop => Msg::GitViewCommandStashPop,
            Self::OpenMenu => return None,
            Self::TakeMine => Msg::GitViewConflictTakeMine,
            Self::TakeTheirs => Msg::GitViewConflictTakeTheirs,
            Self::TakeBoth => Msg::GitViewConflictTakeBoth,
            Self::OpenEditor => Msg::GitViewConflictOpenEditor,
            Self::MarkResolved => Msg::GitViewConflictMarkResolved,
            Self::ResetConflict => Msg::GitViewCommandConflictReset,
            Self::RepoPicker => Msg::GitViewCommandRepoPicker,
            Self::BranchPicker => Msg::GitViewCommandBranchPicker,
            Self::JumpHead => Msg::GitViewCommandJumpHead,
            Self::JumpWip => Msg::GitViewCommandJumpWip,
            Self::CopySha => Msg::GitViewCommandCopySha,
            Self::GoParent => Msg::GitViewCommandParent,
            Self::ToggleTree => Msg::GitViewCommandToggleTree,
        })
    }

    pub fn message(self) -> Message {
        match self {
            Self::Fetch => Message::Fetch,
            Self::Pull => Message::Pull,
            Self::Push => Message::Push,
            Self::Stash => Message::Stash,
            Self::Undo => Message::Undo,
            Self::Commit => Message::Commit,
            Self::StageAll => Message::StageAll,
            Self::UnstageAll => Message::UnstageAll,
            Self::DiscardAll => Message::DiscardAll,
            Self::NewBranch => Message::MenuPressed(MenuAction::NewBranchFrom),
            Self::Checkout => Message::MenuPressed(MenuAction::Checkout),
            Self::Rename => Message::MenuPressed(MenuAction::Rename),
            Self::Delete => Message::MenuPressed(MenuAction::Delete),
            Self::Merge => Message::MenuPressed(MenuAction::MergeInto),
            Self::Rebase => Message::MenuPressed(MenuAction::RebaseOnto),
            Self::StashApply => Message::MenuPressed(MenuAction::StashApply),
            Self::StashPop => Message::MenuPressed(MenuAction::StashApplyAndDrop),
            Self::OpenMenu => Message::OpenMenuKey,
            Self::TakeMine => Message::ConflictShortcut(ConflictAction::Take(Side::Mine)),
            Self::TakeTheirs => Message::ConflictShortcut(ConflictAction::Take(Side::Theirs)),
            Self::TakeBoth => Message::ConflictShortcut(ConflictAction::Take(Side::Both)),
            Self::OpenEditor => Message::ConflictShortcut(ConflictAction::OpenEditor),
            Self::MarkResolved => Message::ConflictShortcut(ConflictAction::MarkResolved),
            Self::ResetConflict => Message::ConflictShortcut(ConflictAction::Reset),
            Self::RepoPicker => Message::OpenPicker(PickerKind::Repository),
            Self::BranchPicker => Message::OpenPicker(PickerKind::Branch),
            Self::JumpHead => Message::JumpToHead,
            Self::JumpWip => Message::JumpToWip,
            Self::CopySha => Message::CopySha,
            Self::GoParent => Message::GoToParent,
            Self::ToggleTree => Message::ToggleTreeKey,
        }
    }

    /// The shortcut a menu entry shows.
    pub fn of_menu(action: MenuAction) -> Self {
        match action {
            MenuAction::Checkout | MenuAction::CheckoutAsLocal => Self::Checkout,
            MenuAction::NewBranchFrom => Self::NewBranch,
            MenuAction::Rename => Self::Rename,
            MenuAction::Delete | MenuAction::DeleteOnRemote | MenuAction::StashDrop => Self::Delete,
            MenuAction::MergeInto => Self::Merge,
            MenuAction::RebaseOnto => Self::Rebase,
            MenuAction::Push => Self::Push,
            MenuAction::StashApply => Self::StashApply,
            MenuAction::StashApplyAndDrop => Self::StashPop,
        }
    }

    pub fn of_conflict(action: ConflictAction) -> Self {
        match action {
            ConflictAction::Take(Side::Mine) => Self::TakeMine,
            ConflictAction::Take(Side::Theirs) => Self::TakeTheirs,
            ConflictAction::Take(Side::Both) => Self::TakeBoth,
            ConflictAction::OpenEditor => Self::OpenEditor,
            ConflictAction::MarkResolved => Self::MarkResolved,
            ConflictAction::Reset => Self::ResetConflict,
        }
    }
}

/// Every Git action the command list offers while a Git tab is shown, with
/// its label and key. The place in the list is what the app sends back to
/// [`message_of_command`].
pub fn commands(lang: Language, keys: &GitKeys) -> Vec<(String, String)> {
    Shortcut::ALL
        .iter()
        .filter_map(|shortcut| {
            let label = lang.text(shortcut.command_label()?).to_owned();
            Some((label, shortcut.label(keys)))
        })
        .collect()
}

/// The message of the command at `place` in [`commands`].
pub fn message_of_command(place: usize) -> Option<Message> {
    Shortcut::ALL
        .iter()
        .filter(|shortcut| shortcut.command_label().is_some())
        .nth(place)
        .map(|shortcut| shortcut.message())
}
