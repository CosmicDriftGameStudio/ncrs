use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use std::collections::VecDeque;

use ncrs_index::{DocumentFields, Kinds, RuleSpec};

use crate::chat::{parse_chat_reply, ChatIntent, ChatModel, ChatTurn};
use crate::contract::{parse_reply, Extraction, ExtractionRequest, Provider};
use crate::error::AiError;
use crate::rules::{ContextDocument, Levels, Mismatch, Revision};
use crate::status::LocalModelStatus;

/// Answers with a canned raw reply, which goes through the same parser as the
/// reply of a real provider, or with a canned error. Remembers what it was
/// asked.
#[derive(Debug)]
pub struct FakeProvider {
    outcome: Result<String, AiError>,
    asked: Mutex<Vec<ExtractionRequest>>,
    answers_now: AtomicBool,
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
            answers_now: AtomicBool::new(true),
        }
    }

    /// Whether the provider can answer now, as a provider does that still
    /// waits for a download or for consent.
    pub fn set_answers_now(&self, answers_now: bool) {
        self.answers_now.store(answers_now, Ordering::Relaxed);
    }

    pub fn requests(&self) -> Vec<ExtractionRequest> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Provider for FakeProvider {
    fn can_answer(&self, _source: &Path) -> bool {
        self.answers_now.load(Ordering::Relaxed)
    }

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
    rules: Mutex<VecDeque<Result<RuleSpec, AiError>>>,
    levels: Mutex<VecDeque<Result<Levels, AiError>>>,
    described: Mutex<Vec<String>>,
    recognized: Mutex<Vec<Vec<(String, DocumentFields)>>>,
    depths: Mutex<Vec<Option<usize>>>,
    revised: Mutex<Vec<(Levels, String)>>,
    revisions: Mutex<VecDeque<Result<Revision, AiError>>>,
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
            rules: Mutex::new(VecDeque::new()),
            levels: Mutex::new(VecDeque::new()),
            described: Mutex::new(Vec::new()),
            recognized: Mutex::new(Vec::new()),
            depths: Mutex::new(Vec::new()),
            revised: Mutex::new(Vec::new()),
            revisions: Mutex::new(VecDeque::new()),
        }
    }

    /// The levels and the sentence each `revise_levels` was asked, in order;
    /// it answers from `push_revision`.
    pub fn revised(&self) -> Vec<(Levels, String)> {
        self.revised
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The next answer of `generate_rule`.
    pub fn push_rule(&self, rule: Result<RuleSpec, AiError>) {
        self.rules
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(rule);
    }

    /// The next answer of `recognize_levels`.
    pub fn push_levels(&self, levels: Result<Levels, AiError>) {
        self.levels
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(levels);
    }

    /// The next answer of `revise_levels`.
    pub fn push_revision(&self, revision: Result<Revision, AiError>) {
        self.revisions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(revision);
    }

    /// The descriptions `generate_rule` was asked, in order.
    pub fn described(&self) -> Vec<String> {
        self.described
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The files each `recognize_levels` was asked about, in order.
    pub fn recognized(&self) -> Vec<Vec<(String, DocumentFields)>> {
        self.recognized
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The folder depth each `recognize_levels` was told, in order.
    pub fn recognized_depths(&self) -> Vec<Option<usize>> {
        self.depths
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
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

    fn generate_rule(
        &self,
        description: &str,
        _documents: &[ContextDocument],
        _kinds: &Kinds,
    ) -> Result<RuleSpec, AiError> {
        self.described
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(description.to_owned());
        self.rules
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
            .unwrap_or(Err(AiError::Model("no scripted rule")))
    }

    fn recognize_levels(
        &self,
        files: &[(String, DocumentFields)],
        _kinds: &Kinds,
        folder_depth: Option<usize>,
    ) -> Result<Levels, AiError> {
        self.recognized
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(files.to_vec());
        self.depths
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(folder_depth);
        self.levels
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
            .unwrap_or(Err(AiError::Model("no scripted levels")))
    }

    fn recognize_levels_again(
        &self,
        files: &[(String, DocumentFields)],
        kinds: &Kinds,
        folder_depth: Option<usize>,
        _previous: &Levels,
        _misfits: &[Mismatch],
    ) -> Result<Levels, AiError> {
        self.recognize_levels(files, kinds, folder_depth)
    }

    fn revise_levels(
        &self,
        _files: &[(String, DocumentFields)],
        _kinds: &Kinds,
        previous: &Levels,
        sentence: &str,
    ) -> Result<Revision, AiError> {
        self.revised
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((previous.clone(), sentence.to_owned()));
        self.revisions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
            .unwrap_or(Err(AiError::Model("no scripted revision")))
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
