//! The thread that owns the model. Callers queue a job and wait for its
//! reply; only this thread touches the model, so there is no lock around it.

use std::collections::VecDeque;
use std::fs;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use ncrs_priority::{set_current_thread_priority, ThreadPriority};

use super::chat_session::ChatSession;
use super::{LocalModel, LocalModelConfig, LOAD_REFUSED};
use crate::chat::{ChatIntent, ChatTurn};
use crate::contract::{Extraction, ExtractionRequest};
use crate::error::AiError;
use crate::model_file::ModelPin;
use crate::status::LocalModelStatus;

/// How long the model stays in memory with nothing to do.
pub const IDLE_UNLOAD: Duration = Duration::from_secs(5 * 60);

#[derive(Debug)]
pub struct ExtractJob {
    pub request: ExtractionRequest,
    pub reply: Sender<Result<Extraction, AiError>>,
}

#[derive(Debug)]
pub struct ChatJob {
    pub turn: ChatTurn,
    pub reply: Sender<Result<ChatIntent, AiError>>,
}

type RuleTask = Box<dyn FnOnce(Result<&LocalModel, AiError>) + Send>;

/// A task on the rule session of the chat. The task runs on the worker with
/// the loaded model, or learns why there is none.
pub struct RuleJob {
    pub task: RuleTask,
}

impl std::fmt::Debug for RuleJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuleJob").finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub enum Job {
    Chat(ChatJob),
    Rule(RuleJob),
    /// A rule-like task nobody waits for: queued with the document reading.
    Background(RuleJob),
    Extract(ExtractJob),
}

impl Job {
    /// Answers the job with an error, for a model that could not be loaded.
    fn fail(self, error: AiError) {
        // A caller that stopped waiting is no one to tell.
        match self {
            Self::Chat(job) => drop(job.reply.send(Err(error))),
            Self::Rule(job) | Self::Background(job) => (job.task)(Err(error)),
            Self::Extract(job) => drop(job.reply.send(Err(error))),
        }
    }
}

/// A person is typing in the chat, a document waits in the pipeline: the
/// person goes first. A background task (one page at a time) comes before the
/// documents, so it moves on while the reading goes on; chat and waiting tasks
/// still go first between pages.
#[derive(Debug, Default)]
pub struct Queue {
    chat: VecDeque<ChatJob>,
    rule: VecDeque<RuleJob>,
    background: VecDeque<RuleJob>,
    extract: VecDeque<ExtractJob>,
    closed: bool,
}

impl Queue {
    pub fn push(&mut self, job: Job) {
        match job {
            Job::Chat(chat) => self.chat.push_back(chat),
            Job::Rule(rule) => self.rule.push_back(rule),
            Job::Background(background) => self.background.push_back(background),
            Job::Extract(extract) => self.extract.push_back(extract),
        }
    }

    pub fn pop(&mut self) -> Option<Job> {
        self.chat
            .pop_front()
            .map(Job::Chat)
            .or_else(|| self.rule.pop_front().map(Job::Rule))
            .or_else(|| self.background.pop_front().map(Job::Background))
            .or_else(|| self.extract.pop_front().map(Job::Extract))
    }
}

/// Size and modification time of the model file, to tell whether a failure
/// it caused is still current.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    len: u64,
    modified: Option<SystemTime>,
}

impl FileStamp {
    fn of(path: &Path) -> Option<Self> {
        let metadata = fs::metadata(path).ok()?;
        metadata.is_file().then(|| Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
        })
    }
}

/// A load that failed, until the file changes or, for a refusal that may be
/// short of memory, until `retry_at`.
struct LoadFailure {
    reason: &'static str,
    stamp: FileStamp,
    retry_at: Option<Instant>,
}

/// How long a refused load waits before the next job tries again.
const LOAD_RETRY_AFTER: Duration = Duration::from_secs(30);

pub fn file_is_there(path: &Path) -> bool {
    FileStamp::of(path).is_some()
}

enum Waited {
    Job(Box<Job>),
    Idle,
    Closed,
}

/// What the callers and the worker thread share.
#[derive(Debug)]
pub struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
    status: Mutex<LocalModelStatus>,
    path: PathBuf,
    pin: ModelPin,
    threads: Mutex<NonZeroU32>,
    extraction_priority: Mutex<ThreadPriority>,
    context_tokens: Mutex<NonZeroU32>,
    idle_unload: Duration,
    /// Handed to every loaded model, set by `close`.
    stop: Arc<AtomicBool>,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    pub fn new(
        path: PathBuf,
        pin: ModelPin,
        threads: NonZeroU32,
        context_tokens: NonZeroU32,
        idle_unload: Duration,
    ) -> Self {
        let status = if file_is_there(&path) {
            LocalModelStatus::NotLoadedYet
        } else {
            LocalModelStatus::NotDownloaded
        };
        Self {
            queue: Mutex::default(),
            wake: Condvar::new(),
            status: Mutex::new(status),
            path,
            pin,
            threads: Mutex::new(threads),
            extraction_priority: Mutex::new(ThreadPriority::Utility),
            context_tokens: Mutex::new(context_tokens),
            idle_unload,
            stop: Arc::default(),
        }
    }

    /// A snapshot; never waits for the worker.
    pub fn status(&self) -> LocalModelStatus {
        locked(&self.status).clone()
    }

    fn set_status(&self, status: LocalModelStatus) {
        *locked(&self.status) = status;
    }

    /// Looks for the file again after a download.
    pub fn refresh(&self) {
        let mut status = locked(&self.status);
        if *status == LocalModelStatus::NotDownloaded && file_is_there(&self.path) {
            *status = LocalModelStatus::NotLoadedYet;
        }
    }

    /// Used from the next document on.
    pub fn set_threads(&self, threads: NonZeroU32) {
        *locked(&self.threads) = threads;
    }

    /// The largest context any caller asked for is used the next time the model
    /// is loaded.
    pub fn raise_context_tokens(&self, context_tokens: NonZeroU32) {
        let mut current = locked(&self.context_tokens);
        *current = (*current).max(context_tokens);
    }

    /// Used from the next document on, except on Linux: the one thread serves
    /// the chat too, and a Linux thread cannot get back from a lowered nice
    /// value without rights, so there the document reading runs at the
    /// priority of the thread.
    pub fn set_extraction_priority(&self, priority: ThreadPriority) {
        *locked(&self.extraction_priority) = priority;
    }

    /// The model thread takes the priority that belongs to the job it is about
    /// to run.
    fn prepare_for(&self, job: &Job) {
        let priority = match job {
            Job::Chat(_) | Job::Rule(_) => ThreadPriority::UserInitiated,
            // A Linux thread cannot get back from a lowered nice value, and
            // this thread serves the chat too: it stays where it is.
            Job::Background(_) | Job::Extract(_) if cfg!(target_os = "linux") => return,
            Job::Background(_) | Job::Extract(_) => *locked(&self.extraction_priority),
        };
        set_current_thread_priority(priority);
    }

    pub fn submit(&self, job: Job) {
        let mut queue = locked(&self.queue);
        if queue.closed {
            drop(queue);
            job.fail(AiError::Model("the model is closed"));
            return;
        }
        queue.push(job);
        drop(queue);
        self.wake.notify_one();
    }

    /// The worker stops, and with it the model leaves the memory. Jobs still
    /// waiting fail, so no caller waits for an answer that never comes.
    pub fn close(&self) {
        let mut waiting = Vec::new();
        {
            let mut queue = locked(&self.queue);
            queue.closed = true;
            self.stop.store(true, Ordering::Relaxed);
            while let Some(job) = queue.pop() {
                waiting.push(job);
            }
        }
        self.wake.notify_all();
        for job in waiting {
            job.fail(AiError::Model("the model is closed"));
        }
    }

    pub fn is_closed(&self) -> bool {
        locked(&self.queue).closed
    }

    fn wait_for_job(&self, idle: Option<Duration>) -> Waited {
        let deadline = idle.map(|span| Instant::now() + span);
        let mut queue = locked(&self.queue);
        loop {
            if queue.closed {
                return Waited::Closed;
            }
            if let Some(job) = queue.pop() {
                return Waited::Job(Box::new(job));
            }
            queue = match deadline {
                None => self
                    .wake
                    .wait(queue)
                    .unwrap_or_else(PoisonError::into_inner),
                Some(limit) => {
                    let left = limit.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return Waited::Idle;
                    }
                    self.wake
                        .wait_timeout(queue, left)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0
                }
            };
        }
    }

    /// Loads the model, or says at once why not. A failure stays until the
    /// file changes: verifying gigabytes per request would stall everything.
    /// Only a refused load of a verified file is tried again after a while.
    fn load(&self, failed: &mut Option<LoadFailure>) -> Result<LocalModel, AiError> {
        let Some(stamp) = FileStamp::of(&self.path) else {
            self.set_status(LocalModelStatus::NotDownloaded);
            return Err(AiError::Model("the local model is not downloaded"));
        };
        if let Some(known) = failed {
            let waiting = known.retry_at.is_none_or(|at| Instant::now() < at);
            if known.stamp == stamp && waiting {
                return Err(AiError::Model(known.reason));
            }
        }
        self.set_status(LocalModelStatus::NotLoadedYet);
        let config = LocalModelConfig {
            path: self.path.clone(),
            threads: *locked(&self.threads),
            context_tokens: *locked(&self.context_tokens),
            stop: Arc::clone(&self.stop),
        };
        match LocalModel::load(config, &self.pin) {
            Ok(model) => {
                *failed = None;
                self.set_status(LocalModelStatus::Loaded {
                    name: model.model_name().to_owned(),
                    memory_bytes: model.memory_bytes(),
                    compute: model.compute().clone(),
                });
                Ok(model)
            }
            Err(error) => {
                let reason = match error {
                    AiError::Model(reason) => reason,
                    _ => "the local model could not be loaded",
                };
                let retry_at = (reason == LOAD_REFUSED).then(|| Instant::now() + LOAD_RETRY_AFTER);
                *failed = Some(LoadFailure {
                    reason,
                    stamp,
                    retry_at,
                });
                self.set_status(LocalModelStatus::Failed(reason));
                Err(AiError::Model(reason))
            }
        }
    }
}

/// The body of the worker thread: asleep until a job comes, load, serve until
/// nothing came for `idle_unload`, let go of the model, sleep again.
pub fn run(shared: &Shared) {
    let mut failed = None;
    let mut waiting_job: Option<Job> = None;
    loop {
        let first = match waiting_job.take() {
            Some(job) => job,
            None => {
                let Waited::Job(boxed) = shared.wait_for_job(None) else {
                    return;
                };
                *boxed
            }
        };
        shared.prepare_for(&first);
        let loaded_context = *locked(&shared.context_tokens);
        match shared.load(&mut failed) {
            Ok(model) => {
                let served = serve_until_idle(shared, &model, first, loaded_context);
                drop(model);
                shared.set_status(LocalModelStatus::NotLoadedYet);
                match served {
                    Served::Closed => return,
                    Served::Idle => {}
                    Served::NeedsLargerContext(job) => waiting_job = Some(job),
                }
            }
            Err(error) => first.fail(error),
        }
    }
}

/// Why the model was let go of.
enum Served {
    /// The app is gone.
    Closed,
    /// Nothing came for a while.
    Idle,
    /// A caller asked for a larger context than the model was loaded with; the
    /// job goes first on the reloaded model.
    NeedsLargerContext(Job),
}

/// Serves jobs with `model`, loaded with a context of `loaded_context` tokens.
fn serve_until_idle(
    shared: &Shared,
    model: &LocalModel,
    first: Job,
    loaded_context: NonZeroU32,
) -> Served {
    // Dropped before the model: the session borrows it.
    let mut chat: Option<ChatSession<'_>> = None;
    let mut next = Some(first);
    loop {
        let job = match next.take() {
            Some(job) => job,
            None => match shared.wait_for_job(Some(shared.idle_unload)) {
                Waited::Job(job) => *job,
                Waited::Idle => return Served::Idle,
                Waited::Closed => return Served::Closed,
            },
        };
        if *locked(&shared.context_tokens) > loaded_context {
            return Served::NeedsLargerContext(job);
        }
        shared.prepare_for(&job);
        match job {
            Job::Extract(extract) => {
                let threads = *locked(&shared.threads);
                let answer = model.extract_with_threads(&extract.request, threads);
                drop(extract.reply.send(answer));
            }
            Job::Chat(request) => {
                let answer = chat_answer(model, &mut chat, &request.turn);
                drop(request.reply.send(answer));
            }
            Job::Rule(rule) | Job::Background(rule) => (rule.task)(Ok(model)),
        }
    }
}

fn chat_answer<'model>(
    model: &'model LocalModel,
    session: &mut Option<ChatSession<'model>>,
    turn: &ChatTurn,
) -> Result<ChatIntent, AiError> {
    if session.is_none() {
        *session = Some(ChatSession::new(model)?);
    }
    match session {
        Some(active) => active.understand(turn),
        None => Err(AiError::Model("the chat could not start")),
    }
}

#[cfg(test)]
mod queue_tests {
    use std::sync::mpsc::channel;
    use std::sync::Arc;

    use ncrs_index::DocumentFields;

    use super::*;

    fn extract_job(text: &str) -> Job {
        Job::Extract(ExtractJob {
            request: ExtractionRequest {
                text: text.to_owned(),
                open_fields: Vec::new(),
                known: DocumentFields::default(),
                source: PathBuf::new(),
                kinds: Arc::default(),
                language: ncrs_index::KindLanguage::default(),
                judge_document: false,
            },
            reply: channel().0,
        })
    }

    fn chat_job(message: &str) -> Job {
        Job::Chat(ChatJob {
            turn: ChatTurn {
                message: message.to_owned(),
                question: None,
                previous_search: None,
                previous_request: None,
                previous_message: None,
            },
            reply: channel().0,
        })
    }

    #[test]
    fn chat_jobs_are_served_before_documents_and_each_kind_in_order() {
        let mut queue = Queue::default();
        queue.push(extract_job("first document"));
        queue.push(extract_job("second document"));
        queue.push(chat_job("first message"));
        queue.push(chat_job("second message"));

        let mut order = Vec::new();
        while let Some(job) = queue.pop() {
            order.push(match job {
                Job::Chat(chat) => chat.turn.message,
                Job::Rule(_) | Job::Background(_) => String::new(),
                Job::Extract(extract) => extract.request.text,
            });
        }

        assert_eq!(
            order,
            [
                "first message",
                "second message",
                "first document",
                "second document"
            ]
        );
    }

    fn rule_job(label: &'static str, log: &Arc<std::sync::Mutex<Vec<&'static str>>>) -> RuleJob {
        let ran = Arc::clone(log);
        RuleJob {
            task: Box::new(move |_model| ran.lock().unwrap().push(label)),
        }
    }

    #[test]
    fn background_tasks_follow_rule_tasks_and_come_before_documents() {
        let ran = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut queue = Queue::default();
        queue.push(extract_job("first document"));
        queue.push(Job::Background(rule_job("background", &ran)));
        queue.push(extract_job("second document"));
        queue.push(Job::Background(rule_job("later background", &ran)));
        queue.push(Job::Rule(rule_job("rule", &ran)));

        let mut order = Vec::new();
        while let Some(job) = queue.pop() {
            match job {
                Job::Rule(rule) | Job::Background(rule) => {
                    (rule.task)(Err(AiError::Model("unused")));
                    order.push(ran.lock().unwrap().last().copied().unwrap().to_owned());
                }
                Job::Extract(extract) => order.push(extract.request.text),
                Job::Chat(_) => panic!("no chat job was queued"),
            }
        }

        assert_eq!(
            order,
            [
                "rule",
                "background",
                "later background",
                "first document",
                "second document"
            ]
        );
    }

    fn closed_shared() -> Shared {
        let shared = Shared::new(
            PathBuf::from("none.gguf"),
            crate::model_file::LOCAL_MODEL,
            std::num::NonZeroU32::MIN,
            std::num::NonZeroU32::MIN,
            Duration::from_secs(1),
        );
        shared.close();
        shared
    }

    #[test]
    fn a_job_for_a_closed_model_fails_at_once() {
        let shared = closed_shared();
        let (reply, answer) = channel();
        shared.submit(Job::Extract(ExtractJob {
            request: match extract_job("x") {
                Job::Extract(extract) => extract.request,
                Job::Chat(_) | Job::Rule(_) | Job::Background(_) => panic!("not an extraction"),
            },
            reply,
        }));

        answer.recv().unwrap().unwrap_err();
    }

    #[test]
    fn closing_fails_the_jobs_still_waiting() {
        let shared = Shared::new(
            PathBuf::from("none.gguf"),
            crate::model_file::LOCAL_MODEL,
            std::num::NonZeroU32::MIN,
            std::num::NonZeroU32::MIN,
            Duration::from_secs(1),
        );
        let (reply, answer) = channel();
        let Job::Extract(mut waiting) = extract_job("x") else {
            panic!("not an extraction");
        };
        waiting.reply = reply;
        shared.submit(Job::Extract(waiting));

        shared.close();

        answer.recv().unwrap().unwrap_err();
    }
}
