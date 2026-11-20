//! The notes tab: Markdown notes in a folder, topics in a tree and notes on
//! files, with its state, messages, `update` and `view`.

pub mod action;
pub mod crypt;
pub mod ctx;
pub mod frontmatter;
pub mod keys;
pub mod message;
pub mod search;
pub mod secure;
pub mod state;
pub mod store;
pub mod sync;
pub mod syncing;
pub mod tree;
pub mod update;
pub mod vault;
pub mod view;

pub use action::Action;
pub use ctx::{Ctx, ViewCtx};
pub use keys::notes_key;
pub use message::{Message, Mode};
pub use secure::apply_settings;
pub use state::{Seal, State};
pub use update::{flush_now, open, reload, update};
pub use view::view;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod secure_tests;
#[cfg(test)]
mod sync_tests;
