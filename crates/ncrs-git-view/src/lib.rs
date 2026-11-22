//! The Git tab: the branches, worktrees, commit graph and diffs of one
//! repository, with its state, messages, `update` and `view`. Read only.

pub mod action;
pub mod actions;
pub mod ctx;
mod diff_layout;
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
pub use keys::git_view_key;
pub use message::Message;
pub use state::State;
pub use update::{open, update};
pub use view::{tab_title, view};

#[cfg(test)]
mod tests;
