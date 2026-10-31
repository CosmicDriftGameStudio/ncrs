use std::sync::{Mutex, PoisonError};

use crate::contract::{parse_reply, Extraction, ExtractionRequest, Provider};
use crate::error::AiError;

/// Answers with a canned raw reply, which goes through the same parser as the
/// reply of a real provider, or with a canned error. Remembers what it was
/// asked.
#[derive(Debug)]
pub struct FakeProvider {
    outcome: Result<String, AiError>,
    asked: Mutex<Vec<ExtractionRequest>>,
}

impl FakeProvider {
    pub fn replying(raw_reply: &str) -> Self {
        Self::with_outcome(Ok(raw_reply.to_owned()))
    }

    pub fn failing(error: AiError) -> Self {
        Self::with_outcome(Err(error))
    }

    fn with_outcome(outcome: Result<String, AiError>) -> Self {
        Self {
            outcome,
            asked: Mutex::new(Vec::new()),
        }
    }

    pub fn requests(&self) -> Vec<ExtractionRequest> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Provider for FakeProvider {
    fn extract(&self, request: &ExtractionRequest) -> Result<Extraction, AiError> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(request.clone());
        match &self.outcome {
            Ok(raw) => parse_reply(raw, &request.open_fields, &request.tax_contexts),
            Err(error) => Err(error.clone()),
        }
    }
}
