use std::sync::{Mutex, PoisonError};

use std::collections::VecDeque;

use crate::chat::{parse_chat_reply, ChatIntent, ChatModel, ChatTurn};
use crate::contract::{parse_reply, Extraction, ExtractionRequest, Provider};
use crate::error::AiError;
use crate::status::LocalModelStatus;

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
            Ok(raw) => parse_reply(raw, request),
            Err(error) => Err(error.clone()),
        }
    }
}

/// A chat model that plays back a script. Each raw reply goes through the
/// real parser, so a test sees what the parser makes of it.
#[derive(Debug)]
pub struct ScriptedChatModel {
    status: Mutex<LocalModelStatus>,
    replies: Mutex<VecDeque<Result<String, AiError>>>,
    turns: Mutex<Vec<ChatTurn>>,
    refreshes: Mutex<usize>,
}

impl ScriptedChatModel {
    pub fn new<'reply>(
        status: LocalModelStatus,
        replies: impl IntoIterator<Item = Result<&'reply str, AiError>>,
    ) -> Self {
        Self {
            status: Mutex::new(status),
            replies: Mutex::new(
                replies
                    .into_iter()
                    .map(|reply| reply.map(str::to_owned))
                    .collect(),
            ),
            turns: Mutex::new(Vec::new()),
            refreshes: Mutex::new(0),
        }
    }

    /// Another raw reply, after those already scripted.
    pub fn push_reply(&self, reply: Result<&str, AiError>) {
        self.replies
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(reply.map(str::to_owned));
    }

    /// The turns it was asked, in order.
    pub fn turns(&self) -> Vec<ChatTurn> {
        self.turns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn set_status(&self, status: LocalModelStatus) {
        *self.status.lock().unwrap_or_else(PoisonError::into_inner) = status;
    }

    pub fn refresh_count(&self) -> usize {
        *self
            .refreshes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

impl ChatModel for ScriptedChatModel {
    fn understand(&self, turn: &ChatTurn) -> Result<ChatIntent, AiError> {
        self.turns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(turn.clone());
        let next = self
            .replies
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
            .unwrap_or(Err(AiError::Model("no scripted reply")));
        parse_chat_reply(&next?, turn)
    }

    fn status(&self) -> LocalModelStatus {
        self.status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn refresh(&self) {
        *self
            .refreshes
            .lock()
            .unwrap_or_else(PoisonError::into_inner) += 1;
    }
}
