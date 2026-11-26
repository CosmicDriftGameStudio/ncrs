//! The filing tab: what the rules make of the files of one source, with the
//! user's changes on top and one run at the end. For a watched source it shows
//! the questions its filing left open.

pub mod action;
pub mod ctx;
pub mod decide;
pub mod folders;
pub mod footer;
pub mod keys;
pub mod left;
pub mod message;
pub mod question;
pub mod right;
pub mod run;
pub mod scroll;
pub mod state;
pub mod texts;
pub mod tree;
pub mod update;
pub mod view;

pub use action::Action;
pub use ctx::{Ctx, ViewCtx};
pub use keys::{filing_key, key_facts, KeyFacts};
pub use message::Message;
pub use state::{
    busy_source, is_running, question_is_open, stop, tab_title, Change, Link, Outcome, Row,
    Session, State,
};
pub use update::{open, update};
pub use view::view;

#[cfg(test)]
mod tests;
