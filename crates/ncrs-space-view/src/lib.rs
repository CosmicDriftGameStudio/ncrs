//! The Platz tab: what takes the space on the drive, sorted into safe, merged
//! worktrees, what the user decides and what is in use. Nested TEA like the other
//! feature crates: `State`, `Message`, `update`, `view`.

mod action;
mod confirm;
mod ctx;
mod decide;
mod keys;
mod log_lines;
mod menu;
mod message;
mod rows;
mod shortcuts;
mod state;
mod tasks;
mod texts;
mod update;
mod view;

pub use action::Action;
pub use confirm::{ConfirmKind, ConfirmState, RunPlan};
pub use ctx::{Ctx, ViewCtx};
pub use decide::{
    DecideGroup, DecideGroupKind, DecideItem, DecideMessage, DecideNotice, DecideReason,
    DecideState, DECIDE_LIST_ID,
};
pub use keys::{space_key, AppKey, KeyMode, KeyOverride, SpaceKeys, SpaceKeysError};
pub use message::{Finished, MenuEntry, Message};
pub use rows::{DisplayRow, Item, RowKind};
pub use shortcuts::{commands, message_of_command, Shortcut};
pub use state::{Measuring, ResultState, Running, State};
pub use update::{open, update, Outcome, FILTER_ID, LIST_ID};
pub use view::{tab_title, view};
