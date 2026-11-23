//! The keys of the Git actions: one table says which chord does what, the
//! key handler, the menus, the tooltips and the status bar read it.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

use ncrs_i18n::{Language, Msg};

use crate::actions::{ConflictAction, MenuAction, Side};
use crate::message::Message;

/// The key of a chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pressed {
    Letter(&'static str),
    Named(Named),
}

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
}

impl Shortcut {
    pub const ALL: [Self; 24] = [
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
    ];

    /// ⇧⌘ plus a letter, the page keys of the app (⌘P, ⇧⌘P, ⇧⌘F, ⇧⌘B, ⇧⌘D,
    /// ⇧⌘H, ⌘K, ⌘,) stay free.
    fn chord(self) -> (Modifiers, Pressed) {
        let shifted = Modifiers::COMMAND | Modifiers::SHIFT;
        match self {
            Self::Fetch => (shifted, Pressed::Letter("e")),
            Self::Pull => (shifted, Pressed::Letter("l")),
            Self::Push => (shifted, Pressed::Letter("u")),
            Self::Stash => (shifted, Pressed::Letter("s")),
            Self::Undo => (shifted, Pressed::Letter("y")),
            Self::Commit => (Modifiers::COMMAND, Pressed::Named(Named::Enter)),
            Self::StageAll => (shifted, Pressed::Letter("i")),
            Self::UnstageAll => (shifted, Pressed::Letter("r")),
            Self::DiscardAll => (shifted, Pressed::Named(Named::Backspace)),
            Self::NewBranch => (shifted, Pressed::Letter("t")),
            Self::Checkout => (shifted, Pressed::Letter("o")),
            Self::Rename => (Modifiers::SHIFT, Pressed::Named(Named::F6)),
            Self::Delete => (Modifiers::COMMAND, Pressed::Named(Named::Backspace)),
            Self::Merge => (shifted, Pressed::Letter("m")),
            Self::Rebase => (shifted, Pressed::Letter("x")),
            Self::StashApply => (shifted, Pressed::Named(Named::Enter)),
            Self::StashPop => (shifted, Pressed::Letter("w")),
            Self::OpenMenu => (Modifiers::SHIFT, Pressed::Named(Named::F10)),
            Self::TakeMine => (shifted, Pressed::Named(Named::ArrowLeft)),
            Self::TakeTheirs => (shifted, Pressed::Named(Named::ArrowRight)),
            Self::TakeBoth => (shifted, Pressed::Named(Named::ArrowDown)),
            Self::OpenEditor => (Modifiers::empty(), Pressed::Named(Named::F4)),
            Self::MarkResolved => (shifted, Pressed::Letter("j")),
            Self::ResetConflict => (shifted, Pressed::Letter("z")),
        }
    }

    /// The modifiers and key of the chord, as the app-wide keymap spells them.
    pub fn binding(self) -> (Modifiers, Key) {
        let (held, pressed) = self.chord();
        let key = match pressed {
            Pressed::Letter(letter) => Key::Character(letter.into()),
            Pressed::Named(named) => Key::Named(named),
        };
        (held, key)
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

    /// The shortcut a press stands for.
    pub fn pressed(key: &Key, modifiers: Modifiers) -> Option<Self> {
        Self::ALL.into_iter().find(|shortcut| {
            let (held, pressed) = shortcut.chord();
            modifiers == held
                && match (pressed, key.as_ref()) {
                    (Pressed::Letter(letter), Key::Character(typed)) => {
                        typed.eq_ignore_ascii_case(letter)
                    }
                    (Pressed::Named(wanted), Key::Named(typed)) => wanted == typed,
                    _ => false,
                }
        })
    }

    /// The chord as this platform spells it: `⇧⌘E` on a Mac, `Ctrl+Shift+E`
    /// elsewhere.
    pub fn label(self) -> String {
        let (held, pressed) = self.chord();
        let key = match pressed {
            Pressed::Letter(letter) => letter.to_uppercase(),
            Pressed::Named(Named::Enter) => "\u{21b5}".to_owned(),
            Pressed::Named(Named::Backspace) => "\u{232b}".to_owned(),
            Pressed::Named(Named::ArrowLeft) => "\u{2190}".to_owned(),
            Pressed::Named(Named::ArrowRight) => "\u{2192}".to_owned(),
            Pressed::Named(Named::ArrowDown) => "\u{2193}".to_owned(),
            Pressed::Named(named) => format!("{named:?}"),
        };
        let mut label = String::new();
        if cfg!(target_os = "macos") {
            if held.shift() {
                label.push('\u{21e7}');
            }
            if held.command() {
                label.push('\u{2318}');
            }
            label.push_str(&key);
        } else {
            if held.command() {
                label.push_str("Ctrl+");
            }
            if held.shift() {
                label.push_str("Shift+");
            }
            label.push_str(&key);
        }
        label
    }
}

/// Every Git action the command list offers while a Git tab is shown, with
/// its label and key. The place in the list is what the app sends back to
/// [`message_of_command`].
pub fn commands(lang: Language) -> Vec<(String, String)> {
    Shortcut::ALL
        .into_iter()
        .filter_map(|shortcut| {
            let label = lang.text(shortcut.command_label()?).to_owned();
            Some((label, shortcut.label()))
        })
        .collect()
}

/// The message of the command at `place` in [`commands`].
pub fn message_of_command(place: usize) -> Option<Message> {
    Shortcut::ALL
        .into_iter()
        .filter(|shortcut| shortcut.command_label().is_some())
        .nth(place)
        .map(Shortcut::message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_chord_is_its_own() {
        for (index, left) in Shortcut::ALL.iter().enumerate() {
            for right in &Shortcut::ALL[index + 1..] {
                assert_ne!(left.chord(), right.chord(), "{left:?} and {right:?}");
            }
        }
    }

    #[test]
    fn a_chord_finds_its_shortcut() {
        for shortcut in Shortcut::ALL {
            let (held, pressed) = shortcut.chord();
            let key = match pressed {
                Pressed::Letter(letter) => Key::Character(letter.into()),
                Pressed::Named(named) => Key::Named(named),
            };
            assert_eq!(Shortcut::pressed(&key, held), Some(shortcut));
        }
    }
}
