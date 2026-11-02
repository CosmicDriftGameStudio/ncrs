use std::collections::VecDeque;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use ncrs_intake::STABLE_AFTER;

use crate::load::Load;

/// Length of `Snapshot::documents_per_minute`.
pub const RATE_MINUTES: usize = 10;
/// The errors kept for `Snapshot::errors`; the oldest are dropped.
pub const MAX_ERRORS: usize = 50;
/// The entries kept for `Snapshot::changed_paths`; the oldest are dropped.
pub const MAX_CHANGED_PATHS: usize = 512;

const DEFAULT_RESCAN_EVERY: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone)]
pub struct Settings {
    pub load: Load,
    pub logical_cores: NonZeroUsize,
    /// How long a step that was not ready waits before it is tried again.
    pub retry_after: Duration,
    /// How often every location is scanned again, to catch what the watcher
    /// missed. `None` switches it off.
    pub rescan_every: Option<Duration>,
}

impl Settings {
    pub fn new(load: Load, logical_cores: NonZeroUsize) -> Self {
        Self {
            load,
            logical_cores,
            retry_after: STABLE_AFTER,
            rescan_every: Some(DEFAULT_RESCAN_EVERY),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    /// Starting a worker thread.
    Workers,
    Watch,
    Scan,
    Step(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerState {
    Running,
    Paused,
    Idle,
    Off,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Busy {
    pub file: PathBuf,
    pub task: Task,
    pub started_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerSnapshot {
    pub id: usize,
    pub busy: Option<Busy>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepSnapshot {
    pub name: &'static str,
    /// Queued or waiting to be tried again.
    pub waiting: usize,
    pub done: usize,
    pub failed: usize,
    pub unchanged: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorEntry {
    pub file: PathBuf,
    pub task: Task,
    pub message: String,
    pub at: SystemTime,
    /// The newest change sequence when the error happened: a later change of
    /// the same file that did not fail supersedes the error.
    pub sequence: u64,
}

/// How the pipeline left a file whose state changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeOutcome {
    /// A step changed the file's state and more steps follow.
    Advanced,
    /// The file left the pipeline: filed, planned or waiting for a review.
    Finished,
    /// A file that was processed again on request and found nothing to do.
    Unchanged,
    /// The step failed on this file.
    Failed,
}

/// A file whose state in the index or in the pipeline changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedPath {
    /// Counts up from 1 for the lifetime of a runner.
    pub sequence: u64,
    pub path: PathBuf,
    pub outcome: ChangeOutcome,
}

/// The newest changed paths, bounded. A consumer that remembers the sequence it
/// has seen asks for what came after; it learns from the floor when entries it
/// never saw were dropped.
#[derive(Debug, Default)]
pub(super) struct ChangeLog {
    pub entries: VecDeque<ChangedPath>,
    pub latest: u64,
    /// The highest sequence that was dropped, 0 while nothing was.
    pub floor: u64,
}

impl ChangeLog {
    pub(super) fn push(&mut self, path: &Path, outcome: ChangeOutcome) {
        if self
            .entries
            .back()
            .is_some_and(|last| last.path == path && last.outcome == outcome)
        {
            return;
        }
        self.latest += 1;
        self.entries.push_back(ChangedPath {
            sequence: self.latest,
            path: path.to_path_buf(),
            outcome,
        });
        while self.entries.len() > MAX_CHANGED_PATHS {
            if let Some(dropped) = self.entries.pop_front() {
                self.floor = dropped.sequence;
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub state: RunnerState,
    pub worker_target: usize,
    pub workers: Vec<WorkerSnapshot>,
    pub steps: Vec<StepSnapshot>,
    /// Documents that went through the pipeline, `[0]` being the last minute up
    /// to now and `[i]` the minute `i` before that. A document is an item that
    /// ends with `Finished` or passes the last step with `Continue`; files a
    /// rescan finds unchanged are not counted.
    pub documents_per_minute: Vec<usize>,
    /// Newest last.
    pub errors: Vec<ErrorEntry>,
    /// Files that left the pipeline since the start: finished, unchanged,
    /// failed, or passed the last step.
    pub left_pipeline: usize,
    /// Documents a step has planned a place for since the start.
    pub plans_made: usize,
    /// Files whose state changed, oldest first: taken into the index, planned,
    /// finished or failed. Files a rescan finds unchanged are not listed.
    pub changed_paths: Vec<ChangedPath>,
    /// The sequence of the newest change, 0 before the first.
    pub changed_sequence: u64,
    /// The highest sequence dropped from `changed_paths`: whoever last saw a
    /// lower one has missed changes.
    pub changed_sequence_floor: u64,
}

impl Snapshot {
    /// What there is to show when no runner exists.
    pub fn off() -> Self {
        Self {
            state: RunnerState::Off,
            worker_target: 0,
            workers: Vec::new(),
            steps: Vec::new(),
            documents_per_minute: vec![0; RATE_MINUTES],
            errors: Vec::new(),
            left_pipeline: 0,
            plans_made: 0,
            changed_paths: Vec::new(),
            changed_sequence: 0,
            changed_sequence_floor: 0,
        }
    }

    /// The changes after `seen`, or `None` when some of them were dropped (or
    /// the runner started over) and the caller has to assume everything changed.
    pub fn changes_since(&self, seen: u64) -> Option<impl Iterator<Item = &ChangedPath>> {
        let missed = seen < self.changed_sequence_floor || seen > self.changed_sequence;
        (!missed).then(|| {
            self.changed_paths
                .iter()
                .filter(move |change| change.sequence > seen)
        })
    }

    /// Whether the file changed without failing after the error happened, so
    /// the error no longer describes it.
    pub fn is_superseded(&self, error: &ErrorEntry) -> bool {
        self.changed_paths.iter().any(|change| {
            change.outcome != ChangeOutcome::Failed
                && change.sequence > error.sequence
                && change.path == error.file
        })
    }

    /// Files in the pipeline right now: waiting at a step or being worked on
    /// by one. Scans and the watcher are not files.
    pub fn open_files(&self) -> usize {
        let waiting: usize = self.steps.iter().map(|step| step.waiting).sum();
        let running = self
            .workers
            .iter()
            .filter(|worker| {
                worker
                    .busy
                    .as_ref()
                    .is_some_and(|busy| matches!(busy.task, Task::Step(_)))
            })
            .count();
        waiting + running
    }
}
