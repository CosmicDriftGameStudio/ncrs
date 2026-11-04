//! The Hermes chat: the conversation that reads folders, tidies them and
//! undoes its runs. A state machine on fixed sentences; typed sentences go
//! to the local language model, which only names what the user wants. The
//! work that blocks runs through `ncrs_filing::chat_work`.

pub mod action;
pub mod ctx;
pub mod keys;
pub mod message;
mod state;
mod stay_groups;
mod update;
mod view;

mod decide;
mod greeting;
mod learn;
mod named_folders;
mod patterns;
mod photos;
mod texts;
mod tidy;
mod trial_end;
mod understand;
mod undo;
mod waiting;
mod working;

pub use action::Action;
pub use ctx::{Availability, Ctx};
pub use keys::{chat_key, ChatKey};
pub use message::Message;
pub use state::{
    AreaRow, AreasCard, Chip, ChipReply, CountRow, Entry, FileCard, FolderMention, NewLayout, Part,
    RowOutcome, State, Step, TrialCard, TrialRow,
};
pub use update::update;
pub use view::{focus_input, view, ViewCtx, INPUT_ID, SCROLL_ID};
