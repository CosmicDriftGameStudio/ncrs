//! The filing tab: a source folder planned live into the write folder, with
//! the user's changes on top and one run at the end.

pub mod action;
pub mod ctx;
pub mod footer;
pub mod keys;
pub mod left;
pub mod message;
pub mod right;
pub mod state;
pub mod texts;
pub mod tree;
pub mod update;
pub mod view;

pub use action::Action;
pub use ctx::{Ctx, ViewCtx};
pub use keys::filing_key;
pub use message::{FiledReport, Message};
pub use state::{is_running, stop, tab_title, Session, State};
pub use update::{start, update};
pub use view::view;

#[cfg(test)]
mod tests;
