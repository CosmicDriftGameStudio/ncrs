//! The Repos tab: every repository of the machine in one list, with favourites
//! and bulk actions (fetch, switch to the default branch, pull fast-forward).
//! Nested TEA like the other feature crates: `State`, `Message`, `update`, `view`.

mod action;
mod bulk;
mod cleanup;
mod cleanup_tasks;
mod clone;
mod credential;
mod ctx;
mod keys;
mod log;
mod menu;
mod message;
mod push;
mod resize;
mod rows;
mod run;
mod scan;
mod shortcuts;
mod state;
mod store;
mod summary;
mod tasks;
mod texts;
mod update;
mod view;

pub use action::Action;
pub use bulk::{BulkAction, Executed, Outcome, Ran, SkipReason};
pub use cleanup::{
    Cleanup, CleanupMessage, Item, Line, Offer, OfferReason, Phase, Pick, RepoState, Scan, Undone,
    CLEANUP_LIST_ID,
};
pub use clone::CLONE_URL_ID;
pub use ctx::{Ctx, ViewCtx};
pub use keys::{repos_key, AppKey, KeyOverride, ReposKeys, ReposKeysError};
pub use message::{MenuEntry, Message};
pub use rows::GroupKey;
pub use scan::ScanResult;
pub use shortcuts::{commands, message_of_command, Shortcut};
pub use state::{Run, Scope, State};
pub use store::{Origin, Store, StoreError};
pub use summary::Reading;
pub use update::{open, update, FILTER_ID, LIST_ID, REFRESH_EVERY};
pub use view::{tab_title, view};
