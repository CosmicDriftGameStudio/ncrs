//! Asks an AI provider for the fields of a document that the rules left open.
//! A provider answers with fields only: there is no type through which it
//! could ask for anything else, and what it returns is checked before use.

mod consent;
mod contract;
mod error;
mod http;
mod json;
pub use json::Json;
#[cfg(any(test, feature = "testing"))]
mod testing;

pub use consent::{ConsentLedger, OutgoingPreview};
pub use contract::{
    output_schema, parse_reply, ExtractedField, Extraction, ExtractionRequest, Provider,
    MAX_VALUE_CHARS,
};
pub use error::{AiError, EndpointProblem};
pub use http::{OpenAiCompatible, OpenAiCompatibleConfig, Secret, MAX_REPLY_BYTES};
#[cfg(any(test, feature = "testing"))]
pub use testing::FakeProvider;
