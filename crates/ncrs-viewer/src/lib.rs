//! The viewer tab: one file shown rendered (Markdown) or as text, with its
//! state, messages, `update` and `view`.

pub mod action;
pub mod ctx;
pub mod keys;
pub mod load;
pub mod message;
pub mod state;
pub mod update;
pub mod view;

pub use action::Action;
pub use ctx::{Ctx, ViewCtx};
pub use keys::viewer_key;
pub use load::{is_markdown, MAX_BYTES};
pub use message::{Message, Mode, Scroll};
pub use state::State;
pub use update::{open, update};
pub use view::view;

#[cfg(test)]
mod tests;
