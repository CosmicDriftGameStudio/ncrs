//! Understanding what the user types into the Hermes chat. A model answers
//! with one `ChatIntent` per message; this module builds what it is asked
//! (`prompt`, `grammar`) and checks what it says (`parse`). Not behind the
//! `llama` feature: the chat is written against `ChatModel` and tested with a
//! scripted one.

#[cfg(test)]
mod eval;
mod folder_notes;
mod grammar;
mod intent;
#[cfg(all(test, feature = "llama"))]
mod live;
mod name_proposal;
mod parse;
#[cfg(all(test, feature = "llama"))]
mod scoring;
#[cfg(test)]
pub mod shard;
mod turn;

#[cfg(test)]
mod tests;

pub use folder_notes::{folder_notes_message, parse_folder_notes, FOLDER_NOTES_PROMPT};
pub use grammar::chat_grammar;
pub use intent::{
    ChatIntent, ChatModel, FolderFacts, NameBlock, NameSpec, Order, QuestionKind, Schedule,
    SpaceAsk, Transfer, EVERY_AREA, NAME_SEPARATORS,
};
pub use name_proposal::propose_name_spec;
pub use parse::parse_chat_reply;
pub use turn::{chat_user_message, search_json, ChatQuestion, ChatTurn, SearchQuery};

/// The instructions of the chat model, with the intents and examples of each.
pub const CHAT_SYSTEM_PROMPT: &str = include_str!("prompt.txt");
