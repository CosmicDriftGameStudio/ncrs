//! The menus behind the Pull and Push buttons: which entries they have and
//! which letter picks each.

use ncrs_git::PullStrategy;

/// The button whose menu is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncKind {
    Pull,
    Push,
}

/// An entry of one of the menus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncAction {
    Pull(PullStrategy),
    Push,
    ForcePush,
}

impl SyncAction {
    /// The letter that picks the entry while its menu is open.
    pub const fn letter(self) -> Option<char> {
        match self {
            Self::Pull(PullStrategy::Merge) => Some('m'),
            Self::Pull(PullStrategy::Rebase) => Some('r'),
            Self::Pull(PullStrategy::FastForwardOnly) | Self::ForcePush => Some('f'),
            Self::Push => None,
        }
    }
}

const PULL_ENTRIES: [SyncAction; 3] = [
    SyncAction::Pull(PullStrategy::Merge),
    SyncAction::Pull(PullStrategy::Rebase),
    SyncAction::Pull(PullStrategy::FastForwardOnly),
];
const PUSH_ENTRIES: [SyncAction; 2] = [SyncAction::Push, SyncAction::ForcePush];

impl SyncKind {
    pub const fn entries(self) -> &'static [SyncAction] {
        match self {
            Self::Pull => &PULL_ENTRIES,
            Self::Push => &PUSH_ENTRIES,
        }
    }
}

/// The open menu with the entry the keys are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncMenu {
    pub kind: SyncKind,
    pub cursor: usize,
}

impl SyncMenu {
    pub const fn new(kind: SyncKind) -> Self {
        Self { kind, cursor: 0 }
    }

    pub fn under_cursor(&self) -> Option<SyncAction> {
        self.kind.entries().get(self.cursor).copied()
    }

    /// The entry a letter picks.
    pub fn with_letter(&self, letter: char) -> Option<SyncAction> {
        self.kind
            .entries()
            .iter()
            .copied()
            .find(|entry| entry.letter() == Some(letter.to_ascii_lowercase()))
    }

    /// One step with the arrow keys; the cursor stops at both ends.
    pub fn step(&mut self, down: bool) {
        let last = self.kind.entries().len().saturating_sub(1);
        self.cursor = if down {
            (self.cursor + 1).min(last)
        } else {
            self.cursor.saturating_sub(1)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_pick_their_entry_in_either_case() {
        let menu = SyncMenu::new(SyncKind::Pull);
        assert_eq!(
            menu.with_letter('R'),
            Some(SyncAction::Pull(PullStrategy::Rebase))
        );
        assert_eq!(menu.with_letter('x'), None);
        assert_eq!(
            SyncMenu::new(SyncKind::Push).with_letter('f'),
            Some(SyncAction::ForcePush)
        );
    }

    #[test]
    fn the_cursor_stops_at_the_ends() {
        let mut menu = SyncMenu::new(SyncKind::Push);
        menu.step(false);
        assert_eq!(menu.cursor, 0);
        menu.step(true);
        menu.step(true);
        assert_eq!(menu.cursor, 1);
    }
}
