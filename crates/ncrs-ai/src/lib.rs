//! Asks an AI provider for the fields of a document that the rules left open.
//! A provider answers with fields only: there is no type through which it
//! could ask for anything else, and what it returns is checked before use.

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
#[cfg(any(test, feature = "testing"))]
mod testing;

pub use consent::{ConsentLedger, NoConsentGiven, OutgoingPreview};
pub use contract::{
    output_schema, parse_reply, ExtractedField, Extraction, ExtractionRequest, Provider,
    MAX_VALUE_CHARS,
};
pub use error::{AiError, EndpointProblem};
pub use http::{
    check_endpoint, endpoint_is_local, endpoint_origin, OpenAiCompatible, OpenAiCompatibleConfig,
    Secret, MAX_REPLY_BYTES,
};
#[cfg(feature = "llama")]
pub use local::{LazyLocalModel, LocalModel, LocalModelConfig, LocalModelStatus};
pub use model_file::{
    has_partial_download, verify_model_file, ModelFileError, ModelPin, PartialDownload,
    LOCAL_MODEL, LOCAL_MODEL_FOLDER,
};
#[cfg(any(test, feature = "testing"))]
pub use testing::FakeProvider;
