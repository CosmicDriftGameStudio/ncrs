//! The Hermes chat: the conversation that reads folders, tidies them and
//! undoes its runs. A state machine on fixed sentences; typed sentences go
//! to the local language model, which only names what the user wants. The
//! work that blocks runs through `ncrs_filing::chat_work`.

pub mod action;
mod chat_list;
mod chat_list_update;
mod chat_store;
pub mod ctx;
pub mod keys;
mod list_view;
pub mod message;
mod state;
mod update;
mod view;

mod decide;
mod fallback;
mod follow_up;
mod greeting;
mod in_tab;
mod learn;
mod named_folders;
mod photos;
mod picks;
mod rule_flow;
mod texts;
mod tidy;
mod trial_end;
mod understand;
mod undo;
mod waiting;
mod working;

pub use action::Action;
pub use chat_list::{day_group, ChatList, DayGroup, ListGroup, ListRow};
pub use chat_list_update::ListMessage;
pub use chat_store::{ChatId, ChatStatus};
pub use ctx::{Availability, Ctx};
pub use in_tab::{filed_in_tab, FiledInTab};
pub use keys::{chat_key, ChatKey};
pub use message::Message;
pub use rule_flow::{RuleAnswer, RuleAsk};
pub use state::{
    AreaRow, AreasCard, Chip, ChipReply, CountRow, Entry, FileCard, FolderMention, LevelsCard,
    NewLayout, Part, ProposalCard, State, Step,
};
pub use texts::problem_text;
pub use update::update;
pub use view::{focus_input, view, ViewCtx, INPUT_ID, SCROLL_ID};
