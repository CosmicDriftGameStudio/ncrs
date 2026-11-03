//! The thread that owns the model. Callers queue a job and wait for its
//! reply; only this thread touches the model, so there is no lock around it.

use std::collections::VecDeque;
use std::fs;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use super::chat_session::ChatSession;
use super::{LocalModel, LocalModelConfig};
use crate::chat::{ChatIntent, ChatTurn};
use crate::contract::{Extraction, ExtractionRequest, Provider as _};
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

#[derive(Debug)]
pub enum Job {
    Chat(ChatJob),
    Extract(ExtractJob),
}

impl Job {
    /// Answers the job with an error, for a model that could not be loaded.
    fn fail(self, error: AiError) {
        // A caller that stopped waiting is no one to tell.
        match self {
            Self::Chat(job) => drop(job.reply.send(Err(error))),
            Self::Extract(job) => drop(job.reply.send(Err(error))),
        }
    }
}

/// A person is typing in the chat, a document waits in the pipeline: the
/// person goes first.
#[derive(Debug, Default)]
pub struct Queue {
    chat: VecDeque<ChatJob>,
    extract: VecDeque<ExtractJob>,
    closed: bool,
}

impl Queue {
    pub fn push(&mut self, job: Job) {
        match job {
            Job::Chat(chat) => self.chat.push_back(chat),
            Job::Extract(extract) => self.extract.push_back(extract),
        }
    }

    pub fn pop(&mut self) -> Option<Job> {
        self.chat
            .pop_front()
            .map(Job::Chat)
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
    context_tokens: NonZeroU32,
    idle_unload: Duration,
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
            context_tokens,
            idle_unload,
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

    /// Used at the next load.
    pub fn set_threads(&self, threads: NonZeroU32) {
        *locked(&self.threads) = threads;
    }

    pub fn submit(&self, job: Job) {
        locked(&self.queue).push(job);
        self.wake.notify_one();
    }

    /// The worker stops, and with it the model leaves the memory.
    pub fn close(&self) {
        locked(&self.queue).closed = true;
        self.wake.notify_all();
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
    fn load(&self, failed: &mut Option<(&'static str, FileStamp)>) -> Result<LocalModel, AiError> {
        let Some(stamp) = FileStamp::of(&self.path) else {
            self.set_status(LocalModelStatus::NotDownloaded);
            return Err(AiError::Model("the local model is not downloaded"));
        };
        if let Some((reason, failed_stamp)) = failed {
            if *failed_stamp == stamp {
                return Err(AiError::Model(reason));
            }
        }
        self.set_status(LocalModelStatus::NotLoadedYet);
        let config = LocalModelConfig {
            path: self.path.clone(),
            threads: *locked(&self.threads),
            context_tokens: self.context_tokens,
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
                *failed = Some((reason, stamp));
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
    loop {
        let Waited::Job(boxed) = shared.wait_for_job(None) else {
            return;
        };
        let first = *boxed;
        match shared.load(&mut failed) {
            Ok(model) => {
                let closed = serve_until_idle(shared, &model, first);
                drop(model);
                if closed {
                    return;
                }
                shared.set_status(LocalModelStatus::NotLoadedYet);
            }
            Err(error) => first.fail(error),
        }
    }
}

/// Serves jobs with `model`; true when the app is gone, false when it was idle.
fn serve_until_idle(shared: &Shared, model: &LocalModel, first: Job) -> bool {
    // Dropped before the model: the session borrows it.
    let mut chat: Option<ChatSession<'_>> = None;
    let mut next = Some(first);
    loop {
        let job = match next.take() {
            Some(job) => job,
            None => match shared.wait_for_job(Some(shared.idle_unload)) {
                Waited::Job(job) => *job,
                Waited::Idle => return false,
                Waited::Closed => return true,
            },
        };
        match job {
            Job::Extract(extract) => drop(extract.reply.send(model.extract(&extract.request))),
            Job::Chat(request) => {
                let answer = chat_answer(model, &mut chat, &request.turn);
                drop(request.reply.send(answer));
            }
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
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
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
}
