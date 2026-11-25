//! The Git tab: the branches, worktrees, commit graph and diffs of one
//! repository, with its state, messages, `update` and `view`. Read only.

pub mod action;
pub mod actions;
pub mod cleanup;
pub mod credentials;
pub mod ctx;
pub mod commit_menu;
mod diff_layout;
pub mod file_views;
pub mod git_keys;
mod handlers;
pub mod job;
pub mod keys;
mod load;
pub mod message;
mod navigate;
pub mod panes;
pub mod picker;
pub mod shortcuts;
pub mod snapshot;
pub mod state;
mod sync_handlers;
pub mod sync_menu;
pub mod update;
pub mod view;
mod wip;

pub use action::Action;
pub use cleanup::CleanupMessage;
pub use ctx::{Ctx, ViewCtx};
pub use git_keys::{GitKeys, GitKeysError, KeyOverride};
pub use keys::{commit_field_key, git_view_key, KeyContext};
pub use message::Message;
pub use panes::{PaneWidths, Split};
pub use picker::{PickerKind, RepoEntry};
pub use shortcuts::{commands, message_of_command};
pub use state::State;
pub use update::{open, update};
pub use view::{tab_title, view};

/// Whether the tab offers the write actions: git is there and the repository was read.
pub fn actions_available(state: &State) -> bool {
    state.actions.unavailable.is_none() && state.actions.local.is_some()
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod wip_tests;
