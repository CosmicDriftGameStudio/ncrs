//! What the confirm dialog holds: what goes, and whether an unknown answer about
//! running tools was accepted.

use ncrs_i18n::{Language, Msg};
use ncrs_space::{Entry, UnknownUse, WorktreeSpace};
use ncrs_ui_kit::confirm::ConfirmFocus;

use crate::texts::{fill_count, fill_size};

/// Which button opened the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmKind {
    /// "Delete all safe ones"; the worktrees can be added with the checkbox.
    Safe,
    /// "Remove merged worktrees".
    Worktrees,
    /// "Delete only this one".
    One,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmState {
    pub kind: ConfirmKind,
    pub safe: Vec<Entry>,
    /// Removed with the run only when `with_worktrees` is set, or when the kind is `Worktrees`.
    pub worktrees: Vec<WorktreeSpace>,
    pub with_worktrees: bool,
    /// ncrs cannot tell whether a build runs: the run needs this confirm to go on.
    pub unknown_use: bool,
    pub focus: ConfirmFocus,
    /// The words on the confirm button; the dialog borrows them, so they live here.
    pub confirm_label: String,
}

/// What a run deletes, as it was confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunPlan {
    pub safe: Vec<Entry>,
    pub worktrees: Vec<WorktreeSpace>,
    pub unknown: UnknownUse,
}

impl ConfirmState {
    /// The dialog has the box "remove merged worktrees too".
    pub fn offers_worktrees(&self, store_build: bool) -> bool {
        self.kind == ConfirmKind::Safe && !store_build && !self.worktrees.is_empty()
    }

    /// A dialog with the focus on Cancel.
    pub fn new(
        kind: ConfirmKind,
        safe: Vec<Entry>,
        worktrees: Vec<WorktreeSpace>,
        unknown_use: bool,
        lang: Language,
    ) -> Self {
        let mut state = Self {
            kind,
            safe,
            worktrees,
            with_worktrees: false,
            unknown_use,
            focus: ConfirmFocus::Cancel,
            confirm_label: String::new(),
        };
        state.refresh_label(lang);
        state
    }

    /// Names the confirm button again after what goes has changed.
    pub fn refresh_label(&mut self, lang: Language) {
        let bytes = self.total_bytes();
        self.confirm_label = match (self.kind, self.unknown_use) {
            (_, true) => fill_size(Msg::SpaceConfirmUnknownUseButton, bytes, lang),
            (ConfirmKind::Worktrees, false) => {
                fill_count(Msg::SpaceConfirmWorktreesButton, self.count(), lang)
            }
            (ConfirmKind::Safe | ConfirmKind::One, false) => {
                fill_size(Msg::SpaceConfirmButton, bytes, lang)
            }
        };
    }

    /// The worktrees the run takes along.
    pub fn worktrees_taken(&self) -> &[WorktreeSpace] {
        if self.with_worktrees
            || self.kind == ConfirmKind::Worktrees
            || self.kind == ConfirmKind::One
        {
            &self.worktrees
        } else {
            &[]
        }
    }

    pub fn plan(&self) -> RunPlan {
        RunPlan {
            safe: self.safe.clone(),
            worktrees: self.worktrees_taken().to_vec(),
            unknown: if self.unknown_use {
                UnknownUse::AllowAfterConfirm
            } else {
                UnknownUse::Skip
            },
        }
    }

    pub fn safe_bytes(&self) -> u64 {
        self.safe
            .iter()
            .fold(0, |sum, entry| sum.saturating_add(entry.size.bytes))
    }

    pub fn worktree_bytes(&self) -> u64 {
        self.worktrees_taken()
            .iter()
            .fold(0, |sum, worktree| sum.saturating_add(worktree.size.bytes))
    }

    pub fn total_bytes(&self) -> u64 {
        self.safe_bytes().saturating_add(self.worktree_bytes())
    }

    pub fn count(&self) -> usize {
        self.safe.len() + self.worktrees_taken().len()
    }
}
