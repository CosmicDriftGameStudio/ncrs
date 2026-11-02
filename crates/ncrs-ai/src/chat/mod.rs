//! Understanding what the user types into the Hermes chat. A model answers
//! with one `ChatIntent` per message; this module builds what it is asked
//! (`prompt`, `grammar`) and checks what it says (`parse`). Not behind the
//! `llama` feature: the chat is written against `ChatModel` and tested with a
//! scripted one.

#[cfg(test)]
pub(crate) mod eval;
mod grammar;
mod intent;
mod parse;
#[cfg(all(test, feature = "llama"))]
pub(crate) mod scoring;
mod turn;

#[cfg(test)]
mod tests;

pub use grammar::chat_grammar;
pub use intent::{ChatIntent, ChatModel, Order, QuestionKind, Schedule, Transfer};
pub use parse::parse_chat_reply;
pub use turn::{chat_user_message, search_json, ChatQuestion, ChatTurn, SearchQuery};

/// The instructions of the chat model, with the intents and examples of each.
pub const CHAT_SYSTEM_PROMPT: &str = include_str!("prompt.txt");
