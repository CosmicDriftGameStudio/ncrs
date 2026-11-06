//! Asks an AI provider for the fields of a document that the rules left open.
//! A provider answers with fields only: there is no type through which it
//! could ask for anything else, and what it returns is checked before use.

mod chat;
mod consent;
mod contract;
mod error;
#[cfg(any(test, feature = "llama"))]
mod grammar;
mod http;
mod json;
pub use json::Json;
#[cfg(feature = "llama")]
mod local;
mod model_file;
mod prompt;
mod rules;
mod status;
#[cfg(any(test, feature = "testing"))]
mod testing;

pub use chat::{
    chat_grammar, chat_user_message, intent_json, parse_chat_reply, search_json, ChatIntent,
    ChatModel, ChatQuestion, ChatTurn, NameBlock, NameSpec, Order, QuestionKind, Schedule,
    SearchQuery, Transfer, CHAT_SYSTEM_PROMPT, EVERY_AREA, NAME_SEPARATORS,
};
pub use consent::{ConsentLedger, NoConsentGiven, OutgoingPreview};
pub use contract::{
    output_schema, parse_reply, ExtractedField, Extraction, ExtractionRequest, Provider, CONFIDENT,
    MAX_VALUE_CHARS,
};
pub use error::{AiError, EndpointProblem};
pub use http::{
    check_endpoint, endpoint_is_local, endpoint_origin, OpenAiCompatible, OpenAiCompatibleConfig,
    Secret, MAX_REPLY_BYTES,
};
#[cfg(feature = "llama")]
pub use local::{
    check_local_model, LazyLocalModel, LocalCheck, LocalModel, LocalModelConfig, RuleSession,
};
pub use model_file::{
    has_partial_download, verify_model_file, ModelFileError, ModelPin, PartialDownload,
    LOCAL_MODEL, LOCAL_MODEL_FOLDER,
};
pub use rules::{
    annotated_paths, condition_meaning, parse_generate, parse_levels, pattern_mismatches,
    prefix_text, ContextDocument, Level, Levels, Mismatch, Part, RuleRequest, END_OF_TURN,
};
pub use status::{Compute, LocalModelStatus};
#[cfg(any(test, feature = "testing"))]
pub use testing::{FakeProvider, ScriptedChatModel};
