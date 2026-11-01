//! The search: the file search engine, the filters over what the index found,
//! the state of the search view, its messages, `update` and `view`.

pub mod action;
pub mod ctx;
pub mod engine;
pub mod filters;
pub mod keys;
pub mod message;
pub mod open;
pub mod state;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
pub mod update;
pub mod view;
pub mod view_state;

pub use action::Action;
pub use ctx::{Ctx, SearchIndex, TogglePress, ToggleShortcuts, ViewCtx};
pub use keys::{search_key, SearchKey, SearchKeyFacts};
pub use message::SearchMessage;
pub use state::{search_events, Search};
pub use update::{activates_panel, needs_keys, update};
pub use view::{field_id, title, view, QUERY_ID};
