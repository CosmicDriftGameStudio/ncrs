//! The local model as the rest of the app holds it: it costs nothing until
//! the first job, loads on its worker thread, and lets go after a quiet time.

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};
use std::thread::{Builder, JoinHandle};
use std::time::Duration;

use ncrs_index::{DocumentFields, Kinds, RuleSpec};
use ncrs_priority::ThreadPriority;

use super::rule_session::RuleSession;
use super::worker::{run, ChatJob, ExtractJob, Job, RuleJob, Shared, IDLE_UNLOAD};
use crate::chat::{ChatIntent, ChatModel, ChatTurn};
use crate::contract::{Extraction, ExtractionRequest, Provider};
use crate::error::AiError;
use crate::model_file::ModelPin;
use crate::rules::{annotated_paths, ContextDocument, Levels};
use crate::status::LocalModelStatus;

const RECOGNIZE_QUESTION: &str = "Which pattern wrote these paths?";

/// The local model, loaded when the first job needs it and only if its file
/// is there. Without the file a call fails at once and does nothing else.
#[derive(Debug)]
pub struct LazyLocalModel {
    shared: Arc<Shared>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl LazyLocalModel {
    pub fn new(
        path: PathBuf,
        pin: ModelPin,
        threads: NonZeroU32,
        context_tokens: NonZeroU32,
    ) -> Self {
        Self::with_idle_unload(path, pin, threads, context_tokens, IDLE_UNLOAD)
    }

    pub fn with_idle_unload(
        path: PathBuf,
        pin: ModelPin,
        threads: NonZeroU32,
        context_tokens: NonZeroU32,
        idle_unload: Duration,
    ) -> Self {
        Self {
            shared: Arc::new(Shared::new(path, pin, threads, context_tokens, idle_unload)),
            worker: Mutex::new(None),
        }
    }

    /// The one model of this process for `path`: field reading and the chat
    /// call this and get the same instance as long as one of them holds it.
    /// A different `threads` is used from the next document on.
    pub fn shared(
        path: PathBuf,
        pin: ModelPin,
        threads: NonZeroU32,
        context_tokens: NonZeroU32,
    ) -> Arc<Self> {
        static REGISTRY: OnceLock<Mutex<HashMap<PathBuf, Weak<LazyLocalModel>>>> = OnceLock::new();
        let mut registry = REGISTRY
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(live) = registry.get(&path).and_then(Weak::upgrade) {
            live.shared.set_threads(threads);
            return live;
        }
        registry.retain(|_, model| model.strong_count() > 0);
        let created = Arc::new(Self::new(path.clone(), pin, threads, context_tokens));
        registry.insert(path, Arc::downgrade(&created));
        created
    }

    /// How the model reads documents from the next one on. The chat is not
    /// affected: it always runs at full priority.
    pub fn set_extraction(&self, threads: NonZeroU32, priority: ThreadPriority) {
        self.shared.set_threads(threads);
        self.shared.set_extraction_priority(priority);
    }

    fn ensure_worker(&self) -> Result<(), AiError> {
        let mut worker = self.worker.lock().unwrap_or_else(PoisonError::into_inner);
        if worker.as_ref().is_some_and(|thread| !thread.is_finished()) {
            return Ok(());
        }
        let shared = Arc::clone(&self.shared);
        let thread = Builder::new()
            .name("ncrs-ai-model".to_owned())
            .spawn(move || run(&shared))
            .map_err(|_refused| AiError::Model("the model thread could not start"))?;
        *worker = Some(thread);
        Ok(())
    }

    /// Runs `work` on a rule session of the worker's model. The session lives
    /// for this one task: no context outlives it.
    // ponytail: the prefix is decoded again for every request; a session cache per folder if the seconds get in the way.
    fn ask_rules<T: Send + 'static>(
        &self,
        documents: &[ContextDocument],
        kinds: &Kinds,
        work: impl FnOnce(&mut RuleSession<'_>) -> Result<T, AiError> + Send + 'static,
    ) -> Result<T, AiError> {
        self.ensure_worker()?;
        let (reply, answer) = channel();
        let owned_documents = documents.to_vec();
        let owned_kinds = kinds.clone();
        self.shared.submit(Job::Rule(RuleJob {
            task: Box::new(move |model| {
                let outcome = model.and_then(|loaded| {
                    let mut session = RuleSession::new(loaded, &owned_documents, &owned_kinds)?;
                    work(&mut session)
                });
                drop(reply.send(outcome));
            }),
        }));
        answer
            .recv()
            .map_err(|_stopped| AiError::Model("the model stopped before it answered"))?
    }

    fn ask<T>(&self, job: impl FnOnce(Sender<Result<T, AiError>>) -> Job) -> Result<T, AiError> {
        self.ensure_worker()?;
        let (reply, answer) = channel();
        self.shared.submit(job(reply));
        answer
            .recv()
            .map_err(|_stopped| AiError::Model("the model stopped before it answered"))?
    }
}

impl Drop for LazyLocalModel {
    fn drop(&mut self) {
        self.shared.close();
    }
}

impl Provider for LazyLocalModel {
    fn can_answer(&self, _source: &Path) -> bool {
        !matches!(
            self.status(),
            LocalModelStatus::NotDownloaded | LocalModelStatus::Failed(_)
        )
    }

    fn extract(&self, request: &ExtractionRequest) -> Result<Extraction, AiError> {
        self.ask(|reply| {
            Job::Extract(ExtractJob {
                request: request.clone(),
                reply,
            })
        })
    }
}

impl ChatModel for LazyLocalModel {
    fn understand(&self, turn: &ChatTurn) -> Result<ChatIntent, AiError> {
        self.ask(|reply| {
            Job::Chat(ChatJob {
                turn: turn.clone(),
                reply,
            })
        })
    }

    fn generate_rule(
        &self,
        description: &str,
        documents: &[ContextDocument],
        kinds: &Kinds,
    ) -> Result<RuleSpec, AiError> {
        let owned_description = description.to_owned();
        self.ask_rules(documents, kinds, move |session| {
            session.generate(&owned_description)
        })
    }

    fn recognize_levels(
        &self,
        files: &[(String, DocumentFields)],
        kinds: &Kinds,
    ) -> Result<Levels, AiError> {
        let documents: Vec<ContextDocument> = files
            .iter()
            .map(|(path, fields)| ContextDocument {
                fields: fields.clone(),
                folder: path.rsplit_once('/').map(|(folder, _)| folder.to_owned()),
            })
            .collect();
        let paths = annotated_paths(files, kinds);
        self.ask_rules(&documents, kinds, move |session| {
            session.recognize(RECOGNIZE_QUESTION, &paths)
        })
    }

    /// Reads a snapshot only, never the disk and never the worker. While the
    /// model is being loaded, or asleep, it reports `NotLoadedYet`.
    fn status(&self) -> LocalModelStatus {
        self.shared.status()
    }

    /// Looks for the file again after a download.
    fn refresh(&self) {
        self.shared.refresh();
    }
}
