//! A queue of long-running file operations.
//!
//! Copying 50.000 files must not freeze the window. `Task::perform` alone cannot
//! do that: the work runs, but the app has no handle on it — no progress, no
//! abort, and a second copy would start on top of the first.
//!
//! iced already has the hard part. `Task::abortable` returns a `Handle` that
//! aborts the future, and `Task::sip` turns a stream of progress into messages.
//! What is missing is the part that decides what runs: one job at a time, the
//! rest waiting.
//!
//! One at a time is deliberate. Two simultaneous copies would share the disk,
//! and the progress line would have to show two bars. Predictable is worth more
//! here than fast.
//!
//! **Not wired up yet.** `App` holds a `Queue` and answers `JobFinished`, but no
//! key enqueues anything and the status bar does not read it yet — that is F5/F6/F8,
//! the next task. The parts that exist before their caller are marked below
//! rather than silenced: a `#[allow(dead_code)]` on a whole type would hide the
//! same warning on the fields that do get used later.

use std::collections::VecDeque;
use std::path::PathBuf;

use crate::messages::Message;

/// What a job does. The label is the i18n key, so the status bar can name it in
/// the active language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum JobKind {
    Copy,
    Move,
    Delete,
    CreateDir,
}

impl JobKind {
    /// TODO(called by the status bar, next task).
    #[allow(dead_code)]
    pub fn label(self) -> crate::i18n::Msg {
        match self {
            JobKind::Copy => crate::i18n::Msg::JobCopy,
            JobKind::Move => crate::i18n::Msg::JobMove,
            JobKind::Delete => crate::i18n::Msg::JobDelete,
            JobKind::CreateDir => crate::i18n::Msg::JobCreateDir,
        }
    }
}

/// One unit of work, with the context the UI needs to describe it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub kind: JobKind,
    /// The directory the job acts in. Shown in the progress line so the user
    /// can see where a long copy is going.
    pub path: PathBuf,
    /// How many items the job will touch, once it knows. `None` while counting,
    /// which is what lets the bar show "counting…" instead of an empty bar.
    pub total: Option<usize>,
}

/// What a job reports while it runs. One `Message` each.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum JobEvent {
    Started {
        job: Job,
    },
    /// Items done, and how many there are in total.
    Progress {
        done: usize,
        total: usize,
    },
    Done,
    /// Failed with a reason, already rendered for the current language: the
    /// filesystem layer has no language, and formatting an error twice would let
    /// the two disagree.
    Failed(String),
    Aborted,
}

/// The job in flight.
#[allow(dead_code)]
struct Running {
    job: Job,
    /// Kept so Escape can stop the work. `Task` has no abort of its own; the
    /// handle `Task::abortable` returns is the only way, and iced does not
    /// re-export its type, so it is stored through a closure-free box that
    /// keeps only what the app needs: the ability to stop.
    abort: Box<dyn Fn()>,
}

/// Jobs waiting their turn.
#[derive(Default)]
pub struct Queue {
    running: Option<Running>,
    waiting: VecDeque<Job>,
}

impl Queue {
    pub fn new() -> Self {
        Self::default()
    }

    /// True while a job runs or jobs are waiting. A new job cannot be queued
    /// while this holds.
    #[allow(dead_code)]
    pub fn is_busy(&self) -> bool {
        self.running.is_some() || !self.waiting.is_empty()
    }

    /// The job in flight, for the progress line.
    /// TODO(called by the status bar, F5/F6/F8).
    #[allow(dead_code)]
    pub fn running(&self) -> Option<&Job> {
        self.running.as_ref().map(|r| &r.job)
    }

    /// How many jobs are waiting behind the current one.
    #[allow(dead_code)]
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    /// Adds a job to the waiting list. The caller starts it by handing the
    /// returned task to `update`, which is what keeps the "does something run
    /// now" decision in one place.
    /// TODO(called when F5/F6/F8 start, next task).
    #[allow(dead_code)]
    pub fn enqueue(&mut self, job: Job) {
        self.waiting.push_back(job);
    }

    /// Takes the next job if nothing is running and returns the task that runs
    /// it. `None` when something is already in flight, which is how the caller
    /// knows the job had to wait.
    ///
    /// The task is returned rather than started here so the one place that
    /// starts work stays `App::update`, where a `Task` can be handed back.
    pub fn start_next(
        &mut self,
        make_task: impl FnOnce(Job) -> iced::Task<Message>,
    ) -> Option<iced::Task<Message>> {
        if self.running.is_some() {
            return None;
        }
        let job = self.waiting.pop_front()?;
        let (task, handle) = make_task(job.clone()).abortable();
        self.running = Some(Running {
            job,
            abort: Box::new(move || handle.abort()),
        });
        Some(task)
    }

    /// Stops the running job. The queue keeps its state, so the caller decides
    /// whether the job restarts or is dropped.
    /// TODO(called by Escape, next task).
    #[allow(dead_code)]
    pub fn abort_running(&mut self) -> bool {
        match &self.running {
            Some(running) => {
                (running.abort)();
                self.running = None;
                true
            }
            None => false,
        }
    }

    /// A job finished. The next one starts, if any.
    pub fn finish(&mut self) {
        self.running = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Task;

    fn job(kind: JobKind) -> Job {
        Job {
            kind,
            path: PathBuf::from("/tmp"),
            total: Some(10),
        }
    }

    #[test]
    fn a_fresh_queue_is_idle() {
        let q = Queue::new();
        assert!(!q.is_busy());
        assert_eq!(q.waiting(), 0);
        assert!(q.running().is_none());
    }

    /// One at a time is the point. A second job waits rather than starting on
    /// top of the first, which is what a copy onto the same disk would do.
    #[test]
    fn a_second_job_waits_instead_of_starting() {
        let mut q = Queue::new();
        q.enqueue(job(JobKind::Copy));

        let started = q.start_next(|_| Task::none());
        assert!(started.is_some(), "the first job should start");
        assert!(
            q.start_next(|_| Task::none()).is_none(),
            "a second job started while one was running"
        );
    }

    /// Finishing frees the slot and the next job takes it.
    #[test]
    fn the_next_job_starts_after_the_first_finishes() {
        let mut q = Queue::new();
        q.enqueue(job(JobKind::Copy));
        q.enqueue(job(JobKind::Move));
        assert_eq!(q.waiting(), 2);

        q.start_next(|_| Task::none());
        q.finish();

        let started = q.start_next(|_| Task::none());
        assert!(started.is_some(), "the queued job should start");
        assert_eq!(q.running().map(|j| j.kind), Some(JobKind::Move));
    }

    /// An empty queue finishes into an idle one, not a panic.
    #[test]
    fn finishing_an_idle_queue_is_harmless() {
        let mut q = Queue::new();
        q.finish();
        assert!(!q.is_busy());
    }

    /// The job carries what the status bar needs: what it does and where.
    #[test]
    fn a_job_names_itself_and_its_path() {
        let j = job(JobKind::Delete);
        assert_eq!(j.path, PathBuf::from("/tmp"));
        assert_eq!(j.kind.label(), crate::i18n::Msg::JobDelete);
    }
}
