//! The Git tab: the branches, worktrees, commit graph and diffs of one
//! repository, with its state, messages, `update` and `view`. Read only.

pub mod action;
pub mod actions;
pub mod ctx;
mod diff_layout;
pub mod git_keys;
mod handlers;
pub mod job;
pub mod keys;
mod load;
pub mod message;
pub mod shortcuts;
pub mod snapshot;
pub mod state;
pub mod update;
pub mod view;

pub use action::Action;
pub use ctx::{Ctx, ViewCtx};
pub use git_keys::{GitKeys, GitKeysError, KeyOverride};
pub use keys::git_view_key;
pub use message::Message;
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
