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
use crate::rules::{
    annotated_paths, repair_note_for, revise_question, ContextDocument, Levels, Mismatch, Revision,
    RECOGNIZE_QUESTION,
};
use crate::status::LocalModelStatus;

/// The local model, loaded when the first job needs it and only if its file
/// is there. Without the file a call fails at once and does nothing else.
#[derive(Debug)]
pub struct LazyLocalModel {
    shared: Arc<Shared>,
}

/// A running worker thread. The process-wide list owns it, not the model, so
/// `shutdown_all` still reaches a worker whose model has been dropped.
#[derive(Debug)]
struct Worker {
    shared: Arc<Shared>,
    thread: JoinHandle<()>,
}

fn workers() -> &'static Mutex<Vec<Worker>> {
    static WORKERS: OnceLock<Mutex<Vec<Worker>>> = OnceLock::new();
    WORKERS.get_or_init(Mutex::default)
}

fn join_all(taken: Vec<Worker>) {
    for worker in &taken {
        worker.shared.close();
    }
    for worker in taken {
        drop(worker.thread.join());
    }
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
        let mut registry = registry().lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(live) = registry.get(&path).and_then(Weak::upgrade) {
            live.shared.set_threads(threads);
            live.shared.raise_context_tokens(context_tokens);
            return live;
        }
        registry.retain(|_, model| model.strong_count() > 0);
        let created = Arc::new(Self::new(path.clone(), pin, threads, context_tokens));
        registry.insert(path, Arc::downgrade(&created));
        created
    }

    /// Ends every model of this process and waits for each worker to free
    /// it, also models that were dropped meanwhile or made by `new`: the app
    /// calls this on quit, because background threads that still hold a model
    /// never drop it and the GPU backend aborts a process that ends with a
    /// model loaded. A running job stops at its next batch or token.
    pub fn shutdown_all() {
        let taken = std::mem::take(&mut *workers().lock().unwrap_or_else(PoisonError::into_inner));
        join_all(taken);
    }

    /// How the model reads documents from the next one on. The chat is not
    /// affected: it always runs at full priority. The priority is not lowered
    /// on Linux.
    pub fn set_extraction(&self, threads: NonZeroU32, priority: ThreadPriority) {
        self.shared.set_threads(threads);
        self.shared.set_extraction_priority(priority);
    }

    /// Closes the model and waits until its worker has freed it. A process
    /// that ends while the worker still frees the model makes the GPU backend
    /// abort, so a test ends with this. Dropping only asks the worker to
    /// stop, which keeps the app from waiting for a running job.
    pub fn shutdown(self: Arc<Self>) {
        self.shared.close();
        let mut list = workers().lock().unwrap_or_else(PoisonError::into_inner);
        let (own, others): (Vec<Worker>, Vec<Worker>) = std::mem::take(&mut *list)
            .into_iter()
            .partition(|worker| Arc::ptr_eq(&worker.shared, &self.shared));
        *list = others;
        drop(list);
        join_all(own);
    }

    fn ensure_worker(&self) -> Result<(), AiError> {
        if self.shared.is_closed() {
            return Err(AiError::Model("the model is closed"));
        }
        let mut list = workers().lock().unwrap_or_else(PoisonError::into_inner);
        list.retain(|worker| !worker.thread.is_finished());
        if list
            .iter()
            .any(|worker| Arc::ptr_eq(&worker.shared, &self.shared))
        {
            return Ok(());
        }
        let shared = Arc::clone(&self.shared);
        let thread = Builder::new()
            .name("ncrs-ai-model".to_owned())
            .spawn(move || run(&shared))
            .map_err(|_refused| AiError::Model("the model thread could not start"))?;
        list.push(Worker {
            shared: Arc::clone(&self.shared),
            thread,
        });
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

    fn documents_of(files: &[(String, DocumentFields)]) -> Vec<ContextDocument> {
        files
            .iter()
            .map(|(path, fields)| ContextDocument {
                fields: fields.clone(),
                folder: path.rsplit_once('/').map(|(folder, _)| folder.to_owned()),
            })
            .collect()
    }

    fn recognize_with(
        &self,
        files: &[(String, DocumentFields)],
        kinds: &Kinds,
        folder_depth: Option<usize>,
        question: String,
    ) -> Result<Levels, AiError> {
        let paths = annotated_paths(files, kinds);
        self.ask_rules(&Self::documents_of(files), kinds, move |session| {
            session.recognize(&question, &paths, folder_depth)
        })
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

type Registry = Mutex<HashMap<PathBuf, Weak<LazyLocalModel>>>;

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
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
        folder_depth: Option<usize>,
    ) -> Result<Levels, AiError> {
        self.recognize_with(files, kinds, folder_depth, RECOGNIZE_QUESTION.to_owned())
    }

    fn recognize_levels_again(
        &self,
        files: &[(String, DocumentFields)],
        kinds: &Kinds,
        folder_depth: Option<usize>,
        previous: &Levels,
        misfits: &[Mismatch],
    ) -> Result<Levels, AiError> {
        let note = repair_note_for(previous, misfits);
        self.recognize_with(
            files,
            kinds,
            folder_depth,
            format!("{RECOGNIZE_QUESTION}\n{note}"),
        )
    }

    fn revise_levels(
        &self,
        files: &[(String, DocumentFields)],
        kinds: &Kinds,
        previous: &Levels,
        sentence: &str,
    ) -> Result<Revision, AiError> {
        let question = revise_question(previous, sentence);
        self.ask_rules(&Self::documents_of(files), kinds, move |session| {
            session.revise(&question)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_file::LOCAL_MODEL;

    fn missing_model() -> Arc<LazyLocalModel> {
        let dir = std::env::temp_dir().join("ncrs-lazy-worker-list");
        Arc::new(LazyLocalModel::new(
            dir.join("missing.gguf"),
            LOCAL_MODEL,
            NonZeroU32::MIN,
            NonZeroU32::new(512).expect("nonzero"),
        ))
    }

    fn workers_of(shared: &Arc<Shared>) -> usize {
        workers()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|worker| Arc::ptr_eq(&worker.shared, shared))
            .count()
    }

    #[test]
    fn a_dropped_model_is_still_joined_on_shutdown_all() {
        let model = missing_model();
        let shared = Arc::clone(&model.shared);
        let turn = ChatTurn {
            message: "hello".to_owned(),
            question: None,
            previous_search: None,
        };
        model.understand(&turn).unwrap_err();
        assert_eq!(workers_of(&shared), 1);
        drop(model);
        LazyLocalModel::shutdown_all();
        assert_eq!(workers_of(&shared), 0);
    }

    #[test]
    fn ensure_worker_twice_keeps_one_worker_and_shutdown_removes_it() {
        let model = missing_model();
        let shared = Arc::clone(&model.shared);
        model.ensure_worker().expect("first worker");
        model.ensure_worker().expect("second call");
        assert_eq!(workers_of(&shared), 1);
        model.shutdown();
        assert_eq!(workers_of(&shared), 0);
    }
}
