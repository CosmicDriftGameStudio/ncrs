//! Asks an AI provider for the fields of a document that the rules left open.
//! A provider answers with fields only: there is no type through which it
//! could ask for anything else, and what it returns is checked before use.

mod api_key;
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
mod usage_log;

pub use api_key::{api_key_account, load_api_key, store_api_key, KeyLookup, KeychainKey};
pub use chat::{
    chat_grammar, chat_user_message, intent_json, parse_chat_reply, propose_name_spec, search_json,
    ChatIntent, ChatModel, ChatQuestion, ChatTurn, NameBlock, NameSpec, Order, QuestionKind,
    Schedule, SearchQuery, Transfer, CHAT_SYSTEM_PROMPT, EVERY_AREA, NAME_SEPARATORS,
};
pub use consent::{ConsentGivenInSettings, ConsentLedger, OutgoingPreview};
pub use contract::{
    output_schema, parse_reply, ExtractedField, Extraction, ExtractionRequest, Provider, CONFIDENT,
    MAX_VALUE_CHARS, NOT_A_DOCUMENT_HINT, NOT_A_DOCUMENT_KEY,
};
pub use error::{AiError, EndpointProblem};
pub use http::{
    check_endpoint, endpoint_host, endpoint_is_local, endpoint_key_name, endpoint_origin,
    list_models, ModelList, OpenAiCompatible, OpenAiCompatibleConfig, ReplyFailure, Secret,
    MAX_REPLY_BYTES,
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
    annotated_paths, condition_meaning, file_names_share_structure, folder_depth, parse_generate,
    parse_levels, parse_revision, pattern_mismatches, prefix_text, repair_note, repair_note_for,
    revise_question, ContextDocument, Level, Levels, Mismatch, Part, Revision, RuleRequest,
    END_OF_TURN,
};
pub use status::{Compute, LocalModelStatus};
#[cfg(any(test, feature = "testing"))]
pub use testing::{FakeProvider, ScriptedChatModel};
pub use usage_log::{
    append_usage, parse_usage, read_usage_log, summarize, usage_log_path, usage_summaries,
    DayUsage, ModelUsage, Totals, Usage, UsageRecord, UsageSummary, CHART_MODELS, USAGE_LOG_FILE,
};
