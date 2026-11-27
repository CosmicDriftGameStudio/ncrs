//! The Hermes chat: the conversation that looks at folders, recognises their
//! order, hands it to the rule suggestions and undoes the last run. It files
//! nothing and saves no rule. A state machine on fixed sentences; typed
//! sentences go to the local language model, which only names what the user
//! wants. The work that blocks runs through `ncrs_filing::chat_work`.

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

mod fallback;
mod learn;
mod named_folders;
mod rule_flow;
mod space_card;
mod texts;
mod understand;
mod undo;
mod working;

pub use action::{Action, SpaceRunKind};
pub use chat_list::{day_group, ChatList, DayGroup, ListGroup, ListRow};
pub use chat_list_update::ListMessage;
pub use chat_store::{ChatId, ChatStatus};
pub use ctx::{Availability, Ctx};
pub use keys::{chat_key, ChatKey};
pub use message::Message;
pub use rule_flow::{RuleAnswer, RuleAsk};
pub use space_card::{SpaceBand, SpaceCard, SpaceRow};
pub use state::{
    AreaRow, AreasCard, Chip, ChipReply, CountRow, Entry, FolderMention, LevelsCard, Part, State,
    Step,
};
pub use texts::problem_text;
pub use update::{card_key_message, update};
pub use view::{focus_input, view, ViewCtx, INPUT_ID, SCROLL_ID};
