use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::fs;
use std::num::NonZeroUsize;
use std::ops::ControlFlow;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use ncrs_intake::{
    classify, walk_location, Found, Location, LocationWatcher, WatchEvent, STABLE_AFTER,
};

use crate::clock::Clock;
use crate::load::Load;
use crate::step::{Item, Step, StepOutcome};

/// Length of `Snapshot::documents_per_minute`.
pub const RATE_MINUTES: usize = 10;
/// The errors kept for `Snapshot::errors`; the oldest are dropped.
pub const MAX_ERRORS: usize = 50;
/// The entries kept for `Snapshot::changed_paths`; the oldest are dropped.
pub const MAX_CHANGED_PATHS: usize = 512;

const PANIC_MESSAGE: &str = "the step stopped unexpectedly";
const DEFAULT_RESCAN_EVERY: Duration = Duration::from_secs(10 * 60);
/// Never shown in practice: items only ever name a registered step.
const UNKNOWN_STEP: &str = "unknown";

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
}

/// A file whose state in the index or in the pipeline changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedPath {
    /// Counts up from 1 for the lifetime of a runner.
    pub sequence: u64,
    pub path: PathBuf,
}

/// The newest changed paths, bounded. A consumer that remembers the sequence it
/// has seen asks for what came after; it learns from the floor when entries it
/// never saw were dropped.
#[derive(Debug, Default)]
struct ChangeLog {
    entries: VecDeque<ChangedPath>,
    latest: u64,
    /// The highest sequence that was dropped, 0 while nothing was.
    floor: u64,
}

impl ChangeLog {
    fn push(&mut self, path: &Path) {
        if self.entries.back().is_some_and(|last| last.path == path) {
            return;
        }
        self.latest += 1;
        self.entries.push_back(ChangedPath {
            sequence: self.latest,
            path: path.to_path_buf(),
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

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct WorkerSlot {
    alive: bool,
    /// Tells a worker's exit guard whether the slot still belongs to it.
    epoch: u64,
    busy: Option<Busy>,
}

#[derive(Debug, Default, Clone, Copy)]
struct StepCounts {
    done: usize,
    failed: usize,
    unchanged: usize,
}

/// Tells a repeated watch report from a change of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fingerprint {
    len: u64,
    modified: SystemTime,
}

impl Fingerprint {
    fn of(path: &Path) -> Option<Self> {
        let metadata = fs::metadata(path).ok()?;
        Some(Self {
            len: metadata.len(),
            modified: metadata.modified().ok()?,
        })
    }
}

#[derive(Debug)]
struct Queued {
    item: Item,
    batch: Option<u64>,
    fingerprint: Option<Fingerprint>,
}

#[derive(Debug)]
struct Continuation {
    item: Item,
    step: usize,
}

#[derive(Debug)]
struct Deferred {
    due: Instant,
    item: Item,
    step: usize,
}

/// The files one scan found. When every one of them has left the first step,
/// the steps get `after_scan`.
#[derive(Debug)]
struct Batch {
    location: Location,
    outstanding: usize,
    settle: bool,
}

enum Job {
    Start,
    Scan(Location),
    Item {
        item: Item,
        step: usize,
        batch: Option<u64>,
    },
}

struct State {
    stopping: bool,
    paused: bool,
    target: usize,
    workers: Vec<WorkerSlot>,
    handles: Vec<JoinHandle<()>>,
    watcher: Option<LocationWatcher>,
    start_pending: bool,
    scans: VecDeque<Location>,
    first_step: VecDeque<Queued>,
    /// Items past the first step. Always picked before new work, so a file
    /// finishes its steps before the next one starts.
    continuations: VecDeque<Continuation>,
    /// Paths queued or deferred at the first step.
    waiting_first: HashSet<PathBuf>,
    /// Each path as it was when it last left the first-step queue since the
    /// latest scan of its location started. Recorded at that point, so a
    /// report arriving while the file runs is matched too.
    taken: HashMap<PathBuf, Fingerprint>,
    deferred: Vec<Deferred>,
    next_rescan: Option<Instant>,
    batches: HashMap<u64, Batch>,
    next_batch: u64,
    counts: Vec<StepCounts>,
    rate: VecDeque<SystemTime>,
    errors: VecDeque<ErrorEntry>,
    left_pipeline: usize,
    plans_made: usize,
    changes: ChangeLog,
}

impl State {
    fn push_error(&mut self, file: PathBuf, task: Task, message: String, at: SystemTime) {
        self.errors.push_back(ErrorEntry {
            file,
            task,
            message,
            at,
        });
        while self.errors.len() > MAX_ERRORS {
            self.errors.pop_front();
        }
    }

    fn queue_scan(&mut self, location: &Location) {
        if !self.scans.iter().any(|queued| queued.path == location.path) {
            self.scans.push_back(location.clone());
        }
    }

    /// False when the path already waits at the first step, which then covers
    /// the change.
    fn queue_first_step(
        &mut self,
        location: &Location,
        path: &Path,
        batch: Option<u64>,
        fingerprint: Option<Fingerprint>,
    ) -> bool {
        if !self.waiting_first.insert(path.to_path_buf()) {
            return false;
        }
        if let Some(entry) = batch.and_then(|id| self.batches.get_mut(&id)) {
            entry.outstanding += 1;
        }
        self.first_step.push_back(Queued {
            item: Item {
                location: location.clone(),
                path: path.to_path_buf(),
                document: None,
                text: None,
                review: None,
                plan: None,
            },
            batch,
            fingerprint,
        });
        true
    }

    fn is_repeated_report(&self, path: &Path, fingerprint: Option<Fingerprint>) -> bool {
        fingerprint.is_some_and(|reported| self.taken.get(path) == Some(&reported))
    }

    fn release(&mut self, batch: u64) -> Option<Location> {
        let entry = self.batches.get_mut(&batch)?;
        entry.outstanding = entry.outstanding.saturating_sub(1);
        if entry.outstanding > 0 {
            return None;
        }
        let finished = self.batches.remove(&batch)?;
        (finished.settle && !self.stopping).then_some(finished.location)
    }

    fn record_document(&mut self, at: SystemTime) {
        let window = Duration::from_secs(60 * RATE_MINUTES as u64);
        self.rate.push_back(at);
        while self
            .rate
            .front()
            .is_some_and(|&oldest| at.duration_since(oldest).is_ok_and(|age| age >= window))
        {
            self.rate.pop_front();
        }
    }

    fn promote_due(&mut self, now: Instant) {
        let mut index = 0;
        while let Some(waiting) = self.deferred.get(index) {
            if waiting.due > now {
                index += 1;
                continue;
            }
            let due_entry = self.deferred.swap_remove(index);
            if due_entry.step == 0 {
                self.first_step.push_back(Queued {
                    item: due_entry.item,
                    batch: None,
                    fingerprint: None,
                });
            } else {
                self.continuations.push_back(Continuation {
                    item: due_entry.item,
                    step: due_entry.step,
                });
            }
        }
    }

    fn next_job(&mut self) -> Option<Job> {
        if let Some(next) = self.continuations.pop_front() {
            return Some(Job::Item {
                item: next.item,
                step: next.step,
                batch: None,
            });
        }
        if self.paused {
            return None;
        }
        if self.start_pending {
            self.start_pending = false;
            return Some(Job::Start);
        }
        if let Some(location) = self.scans.pop_front() {
            // Cleared under the same lock as the pop, so the scan feeds every
            // file again and skips only those taken while it walks.
            self.taken
                .retain(|path, _| !path.starts_with(&location.path));
            return Some(Job::Scan(location));
        }
        let next = self.first_step.pop_front()?;
        self.waiting_first.remove(&next.item.path);
        if let Some(fingerprint) = next.fingerprint {
            self.taken.insert(next.item.path.clone(), fingerprint);
        }
        Some(Job::Item {
            item: next.item,
            step: 0,
            batch: next.batch,
        })
    }

    fn earliest_wakeup(&self) -> Option<Instant> {
        let rescan = if self.paused { None } else { self.next_rescan };
        self.deferred
            .iter()
            .map(|entry| entry.due)
            .chain(rescan)
            .min()
    }

    fn has_queued_work(&self) -> bool {
        self.start_pending
            || !self.scans.is_empty()
            || !self.first_step.is_empty()
            || !self.continuations.is_empty()
            || !self.deferred.is_empty()
    }
}

struct Shared {
    state: Mutex<State>,
    /// Signalled when there may be work or a worker has to exit.
    work_ready: Condvar,
    /// Signalled on every change of the state.
    changed: Condvar,
    steps: Vec<Box<dyn Step>>,
    locations: Vec<Location>,
    clock: Arc<dyn Clock>,
    settings: Settings,
}

/// Marks the slot dead when the worker leaves, including by unwinding.
struct ExitGuard<'shared> {
    shared: &'shared Shared,
    id: usize,
    epoch: u64,
}

impl Drop for ExitGuard<'_> {
    fn drop(&mut self) {
        let (id, epoch) = (self.id, self.epoch);
        self.shared.update(|state| {
            if let Some(slot) = state.workers.get_mut(id) {
                if slot.alive && slot.epoch == epoch {
                    slot.alive = false;
                    slot.busy = None;
                }
            }
        });
        self.shared.work_ready.notify_all();
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn update<R>(&self, change: impl FnOnce(&mut State) -> R) -> R {
        let result = change(&mut self.lock());
        self.changed.notify_all();
        result
    }

    fn is_stopping(&self) -> bool {
        self.lock().stopping
    }

    fn step_name(&self, step: usize) -> &'static str {
        self.steps
            .get(step)
            .map_or(UNKNOWN_STEP, |known| known.name())
    }

    fn ensure_workers(self: &Arc<Self>) {
        let mut state = self.lock();
        if state.stopping {
            return;
        }
        for id in 0..state.target {
            if state.workers.len() <= id {
                state.workers.push(WorkerSlot {
                    alive: false,
                    epoch: 0,
                    busy: None,
                });
            }
            let Some(slot) = state.workers.get_mut(id) else {
                continue;
            };
            if slot.alive {
                continue;
            }
            slot.alive = true;
            slot.epoch += 1;
            let epoch = slot.epoch;
            let shared = Arc::clone(self);
            let spawned = thread::Builder::new()
                .name(format!("ncrs-pipeline-{id}"))
                .spawn(move || worker_main(&shared, id, epoch));
            match spawned {
                Ok(handle) => {
                    state.handles.retain(|known| !known.is_finished());
                    state.handles.push(handle);
                }
                Err(err) => {
                    if let Some(failed_slot) = state.workers.get_mut(id) {
                        failed_slot.alive = false;
                    }
                    let at = self.clock.now();
                    state.push_error(
                        PathBuf::new(),
                        Task::Workers,
                        format!("cannot start a worker: {err}"),
                        at,
                    );
                }
            }
        }
        drop(state);
        self.changed.notify_all();
    }

    fn snapshot(&self) -> Snapshot {
        let state = self.lock();
        self.snapshot_of(&state)
    }

    fn snapshot_of(&self, state: &State) -> Snapshot {
        let now = self.clock.now();
        let runner_state = if state.stopping {
            RunnerState::Off
        } else if state.paused {
            RunnerState::Paused
        } else if state.has_queued_work() || state.workers.iter().any(|slot| slot.busy.is_some()) {
            RunnerState::Running
        } else {
            RunnerState::Idle
        };
        let mut waiting: Vec<usize> = vec![0; self.steps.len()];
        if let Some(first) = waiting.first_mut() {
            *first = state.first_step.len();
        }
        for step in state
            .continuations
            .iter()
            .map(|queued| queued.step)
            .chain(state.deferred.iter().map(|entry| entry.step))
        {
            if let Some(count) = waiting.get_mut(step) {
                *count += 1;
            }
        }
        let steps = self
            .steps
            .iter()
            .enumerate()
            .map(|(position, step)| {
                let counts = state.counts.get(position).copied().unwrap_or_default();
                StepSnapshot {
                    name: step.name(),
                    waiting: waiting.get(position).copied().unwrap_or_default(),
                    done: counts.done,
                    failed: counts.failed,
                    unchanged: counts.unchanged,
                }
            })
            .collect();
        let mut documents_per_minute = vec![0; RATE_MINUTES];
        for &finished_at in &state.rate {
            let age = now.duration_since(finished_at).unwrap_or_default();
            let minute = usize::try_from(age.as_secs().div_euclid(60)).unwrap_or(RATE_MINUTES);
            if let Some(count) = documents_per_minute.get_mut(minute) {
                *count += 1;
            }
        }
        Snapshot {
            state: runner_state,
            worker_target: state.target,
            workers: state
                .workers
                .iter()
                .enumerate()
                .filter(|(_, slot)| slot.alive)
                .map(|(id, slot)| WorkerSnapshot {
                    id,
                    busy: slot.busy.clone(),
                })
                .collect(),
            steps,
            documents_per_minute,
            errors: state.errors.iter().cloned().collect(),
            left_pipeline: state.left_pipeline,
            plans_made: state.plans_made,
            changed_paths: state.changes.entries.iter().cloned().collect(),
            changed_sequence: state.changes.latest,
            changed_sequence_floor: state.changes.floor,
        }
    }

    /// Blocks until there is a job for the worker; `None` when it has to exit.
    /// A worker whose id is at or above the target leaves here, so a job that
    /// is running is never cut short.
    fn next_job_for(&self, id: usize) -> Option<Job> {
        let mut state = self.lock();
        loop {
            if state.stopping || id >= state.target {
                if let Some(slot) = state.workers.get_mut(id) {
                    slot.alive = false;
                    slot.busy = None;
                }
                drop(state);
                self.changed.notify_all();
                return None;
            }
            let now = Instant::now();
            state.promote_due(now);
            if !state.paused {
                if let (Some(every), Some(next)) = (self.settings.rescan_every, state.next_rescan) {
                    if now >= next {
                        for location in &self.locations {
                            state.queue_scan(location);
                        }
                        state.next_rescan = now.checked_add(every);
                    }
                }
            }
            if let Some(job) = state.next_job() {
                let busy = self.busy_for(&job);
                if let Some(slot) = state.workers.get_mut(id) {
                    slot.busy = Some(busy);
                }
                drop(state);
                self.changed.notify_all();
                return Some(job);
            }
            state = match state.earliest_wakeup() {
                Some(due) => {
                    self.work_ready
                        .wait_timeout(state, due.saturating_duration_since(now))
                        .unwrap_or_else(PoisonError::into_inner)
                        .0
                }
                None => self
                    .work_ready
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner),
            };
        }
    }

    fn busy_for(&self, job: &Job) -> Busy {
        let (file, task) = match job {
            Job::Start => (PathBuf::new(), Task::Watch),
            Job::Scan(location) => (location.path.clone(), Task::Scan),
            Job::Item {
                item,
                step,
                batch: _batch,
            } => (item.path.clone(), Task::Step(self.step_name(*step))),
        };
        Busy {
            file,
            task,
            started_at: self.clock.now(),
        }
    }

    fn finish_job(&self, id: usize) {
        self.update(|state| {
            if let Some(slot) = state.workers.get_mut(id) {
                slot.busy = None;
            }
        });
    }

    fn set_busy(&self, id: usize, file: &Path, task: Task) {
        let busy = Busy {
            file: file.to_path_buf(),
            task,
            started_at: self.clock.now(),
        };
        self.update(|state| {
            if let Some(slot) = state.workers.get_mut(id) {
                slot.busy = Some(busy);
            }
        });
    }

    fn record_error(&self, file: &Path, task: Task, message: String) {
        let at = self.clock.now();
        self.update(|state| state.push_error(file.to_path_buf(), task, message, at));
    }

    fn on_watch_event(&self, event: WatchEvent) {
        match event {
            WatchEvent::File(path) => {
                let Some(location) = self
                    .locations
                    .iter()
                    .find(|location| path.starts_with(&location.path))
                else {
                    return;
                };
                let fingerprint = Fingerprint::of(&path);
                // Until the watcher is stored, the scans queued with it still lie
                // ahead and cover the file. Taken here, it would be forgotten when
                // such a scan starts and then fed a second time by its walk.
                let queued = self.update(|state| {
                    !state.stopping
                        && state.watcher.is_some()
                        && !state.is_repeated_report(&path, fingerprint)
                        && state.queue_first_step(location, &path, None, fingerprint)
                });
                if queued {
                    self.work_ready.notify_one();
                }
            }
            WatchEvent::Error { location, message } => {
                let at = self.clock.now();
                let affected_location = self
                    .locations
                    .iter()
                    .find(|candidate| location.starts_with(&candidate.path));
                self.update(|state| {
                    if state.stopping {
                        return;
                    }
                    state.push_error(location.clone(), Task::Watch, message, at);
                    if let Some(found) = affected_location {
                        state.queue_scan(found);
                    }
                });
                self.work_ready.notify_one();
            }
        }
    }

    fn run_after_scan(&self, id: usize, location: &Location) {
        for step in &self.steps {
            if self.is_stopping() {
                return;
            }
            self.set_busy(id, &location.path, Task::Step(step.name()));
            let failed = match catch_unwind(AssertUnwindSafe(|| step.after_scan(location))) {
                Ok(Ok(())) => continue,
                Ok(Err(message)) => message,
                Err(_panic) => PANIC_MESSAGE.to_owned(),
            };
            self.record_error(&location.path, Task::Step(step.name()), failed);
        }
    }

    fn run_scan(&self, id: usize, location: &Location) {
        let batch = self.update(|state| {
            let batch = state.next_batch;
            state.next_batch += 1;
            state.batches.insert(
                batch,
                Batch {
                    location: location.clone(),
                    outstanding: 1,
                    settle: true,
                },
            );
            batch
        });
        let walked = walk_location(location, |found| {
            if self.is_stopping() {
                return ControlFlow::Break(());
            }
            match found {
                Found::File(path) => {
                    if classify(location, path).is_ok() {
                        let fingerprint = Fingerprint::of(path);
                        let queued = self.update(|state| {
                            !state.is_repeated_report(path, fingerprint)
                                && state.queue_first_step(location, path, Some(batch), fingerprint)
                        });
                        if queued {
                            self.work_ready.notify_one();
                        }
                    }
                }
                Found::Hidden => {}
                Found::Unreadable(path) => {
                    self.record_error(path, Task::Scan, "cannot be read".to_owned());
                }
            }
            ControlFlow::Continue(())
        });
        let walk_failed = match walked {
            Ok(()) => false,
            Err(err) => {
                self.record_error(&location.path, Task::Scan, err.to_string());
                true
            }
        };
        let drained = self.update(|state| {
            if walk_failed {
                if let Some(entry) = state.batches.get_mut(&batch) {
                    entry.settle = false;
                }
            }
            state.release(batch)
        });
        if let Some(scanned) = drained {
            self.run_after_scan(id, &scanned);
        }
    }

    fn run_item(&self, id: usize, mut item: Item, step_index: usize, batch: Option<u64>) {
        let name = self.step_name(step_index);
        let outcome = match self.steps.get(step_index) {
            Some(step) => {
                let now = self.clock.now();
                catch_unwind(AssertUnwindSafe(|| step.run(&mut item, now)))
                    .unwrap_or_else(|_panic| Err(PANIC_MESSAGE.to_owned()))
            }
            None => Err("no such step".to_owned()),
        };
        let at = self.clock.now();
        let retry_due = Instant::now().checked_add(self.settings.retry_after);
        let drained = self.update(|state| {
            state.apply(item, step_index, name, outcome, at, retry_due);
            match batch {
                Some(open_batch) if step_index == 0 => state.release(open_batch),
                _ => None,
            }
        });
        self.work_ready.notify_one();
        if let Some(location) = drained {
            self.run_after_scan(id, &location);
        }
    }
}

impl State {
    fn apply(
        &mut self,
        item: Item,
        step_index: usize,
        name: &'static str,
        outcome: Result<StepOutcome, String>,
        at: SystemTime,
        retry_due: Option<Instant>,
    ) {
        let last_step = step_index + 1 >= self.counts.len();
        let Some(counts) = self.counts.get_mut(step_index) else {
            return;
        };
        match outcome {
            Ok(StepOutcome::Continue) => {
                counts.done += 1;
                if step_index == 0 || last_step || item.plan.is_some() || item.review.is_some() {
                    self.changes.push(&item.path);
                }
                if last_step {
                    self.left_pipeline += 1;
                    self.record_document(at);
                } else {
                    self.continuations.push_back(Continuation {
                        item,
                        step: step_index + 1,
                    });
                }
            }
            Ok(StepOutcome::Finished) => {
                counts.done += 1;
                self.left_pipeline += 1;
                if item.plan.is_some() {
                    self.plans_made += 1;
                }
                self.changes.push(&item.path);
                self.record_document(at);
            }
            Ok(StepOutcome::Unchanged) => {
                counts.unchanged += 1;
                self.left_pipeline += 1;
            }
            Ok(StepOutcome::NotReady) => {
                // A change that arrived while this ran is already queued and
                // covers the retry.
                if step_index > 0 || self.waiting_first.insert(item.path.clone()) {
                    self.deferred.push(Deferred {
                        due: retry_due.unwrap_or_else(Instant::now),
                        item,
                        step: step_index,
                    });
                }
            }
            Err(message) => {
                counts.failed += 1;
                self.left_pipeline += 1;
                self.changes.push(&item.path);
                self.push_error(item.path, Task::Step(name), message, at);
            }
        }
    }
}

fn run_start(shared: &Arc<Shared>) {
    let weak: Weak<Shared> = Arc::downgrade(shared);
    let (watcher, failed) = LocationWatcher::start_forwarding(&shared.locations, move |event| {
        if let Some(alive) = weak.upgrade() {
            alive.on_watch_event(event);
        }
    });
    let at = shared.clock.now();
    let unused_watcher = shared.update(|state| {
        for (location, err) in failed {
            state.push_error(location, Task::Watch, err.to_string(), at);
        }
        if state.stopping {
            return Some(watcher);
        }
        state.watcher = Some(watcher);
        for location in &shared.locations {
            state.queue_scan(location);
        }
        None
    });
    // Dropping a watcher joins its thread, which may be waiting for the lock.
    drop(unused_watcher);
    shared.work_ready.notify_all();
}

fn worker_main(shared: &Arc<Shared>, id: usize, epoch: u64) {
    let _exit = ExitGuard { shared, id, epoch };
    while let Some(job) = shared.next_job_for(id) {
        match job {
            Job::Start => run_start(shared),
            Job::Scan(location) => shared.run_scan(id, &location),
            Job::Item { item, step, batch } => shared.run_item(id, item, step, batch),
        }
        shared.finish_job(id);
    }
}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

/// Takes the files of the locations through the steps on a pool of worker
/// threads. Dropping it stops the workers; a step that is running finishes
/// first.
pub struct Runner {
    shared: Arc<Shared>,
}

impl fmt::Debug for Runner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Runner")
            .field("state", &self.shared.snapshot().state)
            .finish_non_exhaustive()
    }
}

impl Runner {
    /// Only spawns the threads; scanning and watching start on a worker.
    pub fn start(
        settings: Settings,
        steps: Vec<Box<dyn Step>>,
        locations: Vec<Location>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let target = settings.load.worker_count(settings.logical_cores).get();
        let state = State {
            stopping: false,
            paused: false,
            target,
            workers: Vec::new(),
            handles: Vec::new(),
            watcher: None,
            start_pending: true,
            scans: VecDeque::new(),
            first_step: VecDeque::new(),
            continuations: VecDeque::new(),
            waiting_first: HashSet::new(),
            taken: HashMap::new(),
            deferred: Vec::new(),
            next_rescan: settings
                .rescan_every
                .and_then(|every| Instant::now().checked_add(every)),
            batches: HashMap::new(),
            next_batch: 0,
            counts: vec![StepCounts::default(); steps.len()],
            rate: VecDeque::new(),
            errors: VecDeque::new(),
            left_pipeline: 0,
            plans_made: 0,
            changes: ChangeLog::default(),
        };
        let shared = Arc::new(Shared {
            state: Mutex::new(state),
            work_ready: Condvar::new(),
            changed: Condvar::new(),
            steps,
            locations,
            clock,
            settings,
        });
        shared.ensure_workers();
        Self { shared }
    }

    /// Running files still pass all their steps; nothing new is started.
    pub fn pause(&self) {
        self.shared.update(|state| state.paused = true);
    }

    pub fn resume(&self) {
        self.shared.update(|state| state.paused = false);
        self.shared.work_ready.notify_all();
    }

    /// Workers that are above the new target leave after their current job.
    pub fn set_load(&self, load: Load) {
        let target = load.worker_count(self.shared.settings.logical_cores).get();
        self.shared.update(|state| state.target = target);
        self.shared.work_ready.notify_all();
        self.shared.ensure_workers();
    }

    /// Reads the runner's own state only, so it is safe on the UI thread while
    /// a worker holds the index.
    pub fn snapshot(&self) -> Snapshot {
        self.shared.snapshot()
    }

    /// Waits for the running steps to finish. Can be called again.
    pub fn stop(&mut self) {
        let (watcher, handles) = self.shared.update(|state| {
            state.stopping = true;
            (state.watcher.take(), std::mem::take(&mut state.handles))
        });
        self.shared.work_ready.notify_all();
        drop(watcher);
        for handle in handles {
            let _ = handle.join();
        }
    }

    #[cfg(test)]
    fn wait_for(&self, what: &str, until: impl Fn(&Snapshot) -> bool) -> Snapshot {
        const HANG_GUARD: Duration = Duration::from_secs(30);
        let deadline = Instant::now() + HANG_GUARD;
        let mut state = self.shared.lock();
        loop {
            let snapshot = self.shared.snapshot_of(&state);
            if until(&snapshot) {
                return snapshot;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(
                !left.is_zero(),
                "timed out waiting for {what}: {snapshot:#?}"
            );
            state = self
                .shared
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use std::collections::HashSet;
    use std::fs;
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::time::UNIX_EPOCH;

    use ncrs_index::{DocumentStatus, Index};
    use ncrs_intake::{
        Extracted, LazyOcrText, LocationMode, PageImage, TextChain, TextError, TextSource,
    };
    use tempfile::TempDir;

    use super::*;
    use crate::step::IndexAccess;
    use crate::steps::intake_steps;

    const HANG_GUARD: Duration = Duration::from_secs(30);
    const INVOICE: &[u8] = b"Invoice 4711 from Company A";
    const REMINDER: &[u8] = b"Reminder 12 from Agency B";
    const CONTRACT: &[u8] = b"Contract 9 from Company A";

    fn cores(count: usize) -> NonZeroUsize {
        NonZeroUsize::new(count).unwrap()
    }

    fn workers(count: usize) -> Load {
        Load::Workers(cores(count))
    }

    fn epoch() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_800_000_000)
    }

    #[derive(Debug)]
    struct FixedClock;

    impl Clock for FixedClock {
        fn now(&self) -> SystemTime {
            epoch()
        }
    }

    /// Moves on with every reading, so files look stable on the second look.
    #[derive(Debug)]
    struct SteppingClock {
        now: Mutex<SystemTime>,
    }

    impl Clock for SteppingClock {
        fn now(&self) -> SystemTime {
            let mut now = self.now.lock().unwrap();
            *now += Duration::from_secs(3);
            *now
        }
    }

    #[derive(Debug, Default)]
    struct GateState {
        everything: bool,
        paths: HashSet<PathBuf>,
    }

    #[derive(Debug, Default)]
    struct Gate {
        state: Mutex<GateState>,
        opened: Condvar,
    }

    impl Gate {
        fn release(&self, path: &Path) {
            self.state.lock().unwrap().paths.insert(path.to_path_buf());
            self.opened.notify_all();
        }

        fn release_everything(&self) {
            self.state.lock().unwrap().everything = true;
            self.opened.notify_all();
        }

        fn wait_until_released(&self, path: &Path) {
            let deadline = Instant::now() + HANG_GUARD;
            let mut state = self.state.lock().unwrap();
            while !state.everything && !state.paths.contains(path) {
                let left = deadline.saturating_duration_since(Instant::now());
                assert!(!left.is_zero(), "{} was never released", path.display());
                state = self.opened.wait_timeout(state, left).unwrap().0;
            }
        }
    }

    type Decide = fn(&Path) -> Result<StepOutcome, String>;

    fn continue_always(_path: &Path) -> Result<StepOutcome, String> {
        Ok(StepOutcome::Continue)
    }

    fn file_name(path: &Path) -> &str {
        path.file_name().and_then(|name| name.to_str()).unwrap()
    }

    /// Reports every start and can hold the item until the test lets it go.
    #[derive(Debug)]
    struct ProbeStep {
        name: &'static str,
        started: Mutex<Sender<PathBuf>>,
        gate: Option<Arc<Gate>>,
        decide: Decide,
    }

    impl Step for ProbeStep {
        fn name(&self) -> &'static str {
            self.name
        }

        fn run(&self, item: &mut Item, _now: SystemTime) -> Result<StepOutcome, String> {
            let _ = self
                .started
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .send(item.path.clone());
            if let Some(gate) = &self.gate {
                gate.wait_until_released(&item.path);
            }
            (self.decide)(&item.path)
        }
    }

    fn probe(
        name: &'static str,
        gate: Option<&Arc<Gate>>,
        decide: Decide,
    ) -> (Box<dyn Step>, Receiver<PathBuf>) {
        let (sender, receiver) = mpsc::channel();
        let step = ProbeStep {
            name,
            started: Mutex::new(sender),
            gate: gate.map(Arc::clone),
            decide,
        };
        (Box::new(step), receiver)
    }

    fn next_started(receiver: &Receiver<PathBuf>) -> PathBuf {
        receiver.recv_timeout(HANG_GUARD).unwrap()
    }

    fn put(location: &Location, name: &str, bytes: &[u8]) -> PathBuf {
        let path = location.path.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    fn location(dir: &TempDir, mode: LocationMode) -> Location {
        Location::new(dir.path(), mode, false).unwrap()
    }

    fn settings(load: Load) -> Settings {
        Settings {
            retry_after: Duration::ZERO,
            rescan_every: None,
            ..Settings::new(load, cores(8))
        }
    }

    fn busy_count(report: &Snapshot) -> usize {
        report
            .workers
            .iter()
            .filter(|worker| worker.busy.is_some())
            .count()
    }

    fn nothing_busy(report: &Snapshot) -> bool {
        busy_count(report) == 0
    }

    fn is_idle(snapshot: &Snapshot) -> bool {
        snapshot.state == RunnerState::Idle
    }

    /// A location with files `file-1.pdf` .. `file-N.pdf`.
    fn numbered_files(count: usize) -> (TempDir, Location) {
        let dir = TempDir::new().unwrap();
        let location = location(&dir, LocationMode::Archive);
        for number in 1..=count {
            put(&location, &format!("file-{number}.pdf"), b"x");
        }
        (dir, location)
    }

    fn start(
        load: Load,
        steps: Vec<Box<dyn Step>>,
        location: &Location,
        clock: Arc<dyn Clock>,
    ) -> Runner {
        Runner::start(settings(load), steps, vec![location.clone()], clock)
    }

    #[test]
    fn a_changed_load_adds_workers_and_lets_the_surplus_finish_its_file() {
        let (_dir, location) = numbered_files(4);
        let gate = Arc::new(Gate::default());
        let (step, started) = probe("gated", Some(&gate), continue_always);
        let runner = start(workers(1), vec![step], &location, Arc::new(FixedClock));

        let first = next_started(&started);
        runner.wait_for("one busy worker", |snapshot| busy_count(snapshot) == 1);
        runner.set_load(workers(3));
        let report = runner.wait_for("three busy workers", |snapshot| busy_count(snapshot) == 3);
        assert_eq!(report.worker_target, 3);
        let running = [first, next_started(&started), next_started(&started)];
        started.try_recv().unwrap_err();

        runner.set_load(workers(1));
        assert_eq!(runner.snapshot().worker_target, 1);
        for path in &running {
            gate.release(path);
        }
        let report_after = runner.wait_for("surplus workers gone", |snapshot| {
            snapshot.workers.len() == 1 && snapshot.steps[0].done == 3
        });
        assert_eq!(report_after.steps[0].failed, 0);

        let fourth = next_started(&started);
        assert!(!running.contains(&fourth));
        assert!(busy_count(&runner.snapshot()) <= 1);
        gate.release(&fourth);
        let report_last = runner.wait_for("all four done", is_idle);
        assert_eq!(report_last.steps[0].done, 4);
    }

    #[test]
    fn pause_lets_running_files_finish_and_starts_no_new_ones() {
        let (_dir, location) = numbered_files(4);
        let gate = Arc::new(Gate::default());
        let (step, started) = probe("gated", Some(&gate), continue_always);
        let runner = start(workers(2), vec![step], &location, Arc::new(FixedClock));

        let running = [next_started(&started), next_started(&started)];
        runner.wait_for("two busy workers", |snapshot| busy_count(snapshot) == 2);
        runner.pause();
        for path in &running {
            gate.release(path);
        }
        let report = runner.wait_for("the running files done", |snapshot| {
            nothing_busy(snapshot) && snapshot.steps[0].done == 2
        });
        assert_eq!(report.state, RunnerState::Paused);
        assert_eq!(report.steps[0].waiting, 2);
        started.try_recv().unwrap_err();

        runner.resume();
        gate.release_everything();
        let report_after = runner.wait_for("the rest done", is_idle);
        assert_eq!(report_after.steps[0].done, 4);
    }

    #[test]
    fn a_paused_runner_still_takes_a_started_file_through_its_remaining_steps() {
        let (_dir, location) = numbered_files(2);
        let gate = Arc::new(Gate::default());
        let (first, first_started) = probe("first", Some(&gate), continue_always);
        let (second, second_started) = probe("second", None, continue_always);
        let runner = start(
            workers(1),
            vec![first, second],
            &location,
            Arc::new(FixedClock),
        );

        let file = next_started(&first_started);
        runner.pause();
        gate.release(&file);
        assert_eq!(next_started(&second_started), file);
        let report = runner.wait_for("the file through both steps", |snapshot| {
            nothing_busy(snapshot) && snapshot.steps[1].done == 1
        });
        assert_eq!(report.state, RunnerState::Paused);
        first_started.try_recv().unwrap_err();

        runner.resume();
        gate.release_everything();
        let report_after = runner.wait_for("both files done", is_idle);
        assert_eq!(report_after.steps[1].done, 2);
    }

    fn fail_the_second_file(path: &Path) -> Result<StepOutcome, String> {
        match file_name(path) {
            "file-2.pdf" => Err("not a readable document".to_owned()),
            _ => Ok(StepOutcome::Continue),
        }
    }

    #[test]
    fn a_failing_file_is_counted_and_listed_and_the_others_carry_on() {
        let (_dir, location) = numbered_files(3);
        let (step, _started) = probe("probe", None, fail_the_second_file);
        let runner = start(workers(2), vec![step], &location, Arc::new(FixedClock));

        let report = runner.wait_for("all three handled", |snapshot| {
            is_idle(snapshot) && snapshot.steps[0].done + snapshot.steps[0].failed == 3
        });

        let counts = &report.steps[0];
        assert_eq!((counts.done, counts.failed, counts.unchanged), (2, 1, 0));
        assert_eq!(report.errors.len(), 1);
        let error = &report.errors[0];
        assert_eq!(error.file, location.path.join("file-2.pdf"));
        assert_eq!(error.task, Task::Step("probe"));
        assert_eq!(error.message, "not a readable document");
        assert_eq!(error.at, epoch());
        assert_eq!(report.documents_per_minute[0], 2);
        assert_eq!(report.documents_per_minute.len(), RATE_MINUTES);
        assert_eq!(report.left_pipeline, 3);
        assert_eq!(report.open_files(), 0);
    }

    /// FSEvents also reports files written shortly before the watch started,
    /// so a file the scan already took can be reported again unchanged.
    #[test]
    fn a_file_reported_again_unchanged_is_not_taken_again() {
        let (_dir, location) = numbered_files(3);
        let (step, _started) = probe("probe", None, fail_the_second_file);
        let runner = start(workers(1), vec![step], &location, Arc::new(FixedClock));
        runner.wait_for("all three handled", |snapshot| {
            is_idle(snapshot) && snapshot.steps[0].done + snapshot.steps[0].failed == 3
        });

        for name in ["file-1.pdf", "file-2.pdf"] {
            runner
                .shared
                .on_watch_event(WatchEvent::File(location.path.join(name)));
        }
        let report = runner.wait_for("the repeated reports handled", is_idle);

        let counts = &report.steps[0];
        assert_eq!((counts.done, counts.failed, counts.unchanged), (2, 1, 0));
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.left_pipeline, 3);
    }

    #[test]
    fn a_failed_file_that_changes_is_tried_again() {
        let (_dir, location) = numbered_files(3);
        let (step, _started) = probe("probe", None, fail_the_second_file);
        let runner = start(workers(1), vec![step], &location, Arc::new(FixedClock));
        runner.wait_for("all three handled", |snapshot| {
            is_idle(snapshot) && snapshot.steps[0].done + snapshot.steps[0].failed == 3
        });

        // Moved in whole, so no report can see the file half written.
        let staging = TempDir::new().unwrap();
        let changed = staging.path().join("file-2.pdf");
        fs::write(&changed, b"a longer body").unwrap();
        let second = location.path.join("file-2.pdf");
        fs::rename(&changed, &second).unwrap();
        runner.shared.on_watch_event(WatchEvent::File(second));
        let report = runner.wait_for("the changed file tried again", |snapshot| {
            is_idle(snapshot) && snapshot.steps[0].failed == 2
        });

        assert_eq!(report.steps[0].done, 2);
        assert_eq!(report.errors.len(), 2);
    }

    fn leave_the_second_file_unchanged(path: &Path) -> Result<StepOutcome, String> {
        match file_name(path) {
            "file-2.pdf" => Ok(StepOutcome::Unchanged),
            _ => Ok(StepOutcome::Finished),
        }
    }

    #[test]
    fn an_unchanged_file_is_neither_done_nor_a_document_in_the_rate() {
        let (_dir, location) = numbered_files(2);
        let (step, _started) = probe("probe", None, leave_the_second_file_unchanged);
        let runner = start(workers(1), vec![step], &location, Arc::new(FixedClock));

        let report = runner.wait_for("both files handled", |snapshot| {
            is_idle(snapshot) && snapshot.steps[0].done + snapshot.steps[0].unchanged == 2
        });

        let counts = &report.steps[0];
        assert_eq!((counts.done, counts.failed, counts.unchanged), (1, 0, 1));
        assert_eq!(report.documents_per_minute[0], 1);
        assert!(report.errors.is_empty());
        assert_eq!(report.left_pipeline, 2);
        assert_eq!(report.open_files(), 0);
    }

    #[test]
    fn open_files_count_waiting_and_step_work_but_not_scans() {
        let busy = |task| WorkerSnapshot {
            id: 0,
            busy: Some(Busy {
                file: PathBuf::from("/tmp/inbox/company-a-invoice.pdf"),
                task,
                started_at: epoch(),
            }),
        };
        let step = |waiting| StepSnapshot {
            name: "probe",
            waiting,
            done: 0,
            failed: 0,
            unchanged: 0,
        };
        let mut snapshot = Snapshot::off();
        snapshot.steps = vec![step(3), step(2)];
        snapshot.workers = vec![
            busy(Task::Scan),
            busy(Task::Watch),
            busy(Task::Workers),
            busy(Task::Step("probe")),
            WorkerSnapshot { id: 4, busy: None },
        ];
        assert_eq!(snapshot.open_files(), 6);
    }

    fn panic_on_the_second_file(path: &Path) -> Result<StepOutcome, String> {
        if file_name(path) == "file-2.pdf" {
            explode();
        }
        Ok(StepOutcome::Continue)
    }

    fn explode() {
        panic!("broken step");
    }

    #[test]
    fn a_panicking_step_is_a_failed_file_and_the_worker_carries_on() {
        let (_dir, location) = numbered_files(3);
        let (step, _started) = probe("probe", None, panic_on_the_second_file);
        let runner = start(workers(1), vec![step], &location, Arc::new(FixedClock));

        let report = runner.wait_for("all three handled", |snapshot| {
            is_idle(snapshot) && snapshot.steps[0].done + snapshot.steps[0].failed == 3
        });

        assert_eq!(report.steps[0].done, 2);
        assert_eq!(report.steps[0].failed, 1);
        assert_eq!(report.errors[0].message, PANIC_MESSAGE);
        assert_eq!(report.errors[0].file, location.path.join("file-2.pdf"));
        assert_eq!(report.workers.len(), 1);
    }

    #[derive(Debug)]
    struct SharedIndex(Arc<Mutex<Index>>);

    impl IndexAccess for SharedIndex {
        fn with_index(&self, work: &mut dyn FnMut(&mut Index)) -> bool {
            work(&mut self.0.lock().unwrap());
            true
        }
    }

    struct IndexFixture {
        _dir: TempDir,
        index: Arc<Mutex<Index>>,
    }

    impl IndexFixture {
        fn new() -> Self {
            let dir = TempDir::new().unwrap();
            let index = Index::create_plain(&dir.path().join("index.db")).unwrap();
            Self {
                _dir: dir,
                index: Arc::new(Mutex::new(index)),
            }
        }

        fn steps(&self) -> Vec<Box<dyn Step>> {
            self.steps_reading(TextChain::new())
        }

        fn steps_reading(&self, chain: TextChain) -> Vec<Box<dyn Step>> {
            intake_steps(
                Arc::new(SharedIndex(Arc::clone(&self.index))),
                Arc::new(chain),
                None,
            )
        }
    }

    fn stepping_clock() -> Arc<dyn Clock> {
        Arc::new(SteppingClock {
            now: Mutex::new(epoch()),
        })
    }

    #[test]
    fn the_backlog_is_taken_in_and_only_inbox_documents_are_reviewed() {
        let inbox_dir = TempDir::new().unwrap();
        let archive_dir = TempDir::new().unwrap();
        let inbox = location(&inbox_dir, LocationMode::Inbox);
        let archive = location(&archive_dir, LocationMode::Archive);
        put(&inbox, "invoice.pdf", INVOICE);
        put(&inbox, "reminder.pdf", REMINDER);
        put(&archive, "contract.pdf", CONTRACT);
        let fixture = IndexFixture::new();
        let runner = Runner::start(
            settings(workers(2)),
            fixture.steps(),
            vec![inbox.clone(), archive.clone()],
            stepping_clock(),
        );

        let report = runner.wait_for("the backlog taken in", |snapshot| {
            is_idle(snapshot) && snapshot.steps[3].done == 3
        });

        assert_eq!(report.steps[0].name, "intake");
        assert_eq!(report.steps[0].done, 3);
        assert_eq!(report.steps[1].name, "text");
        assert_eq!(report.steps[2].name, "rules");
        assert_eq!(report.steps[3].name, "review");
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        let index = fixture.index.lock().unwrap();
        assert_eq!(index.documents_under(&inbox.path).unwrap().len(), 2);
        let archived = index.documents_under(&archive.path).unwrap();
        assert_eq!(archived.len(), 1);
        let reviews = index.open_reviews().unwrap();
        assert_eq!(reviews.len(), 2);
        assert!(reviews
            .iter()
            .all(|review| review.document_id != archived[0].0));
        let document = index.document(archived[0].0).unwrap().unwrap();
        assert_eq!(document.status, DocumentStatus::Filed);
    }

    #[test]
    fn a_new_file_reaches_the_review_list_through_the_watcher() {
        let inbox_dir = TempDir::new().unwrap();
        let inbox = location(&inbox_dir, LocationMode::Inbox);
        put(&inbox, "invoice.pdf", INVOICE);
        let fixture = IndexFixture::new();
        let runner = Runner::start(
            settings(workers(2)),
            fixture.steps(),
            vec![inbox.clone()],
            stepping_clock(),
        );
        runner.wait_for("the backlog reviewed", |snapshot| {
            is_idle(snapshot) && snapshot.steps[3].done == 1
        });

        put(&inbox, "reminder.pdf", REMINDER);

        runner.wait_for("the new file reviewed", |snapshot| {
            snapshot.steps[3].done == 2
        });
        let index = fixture.index.lock().unwrap();
        assert_eq!(index.open_reviews().unwrap().len(), 2);
    }

    #[derive(Debug)]
    struct FixedText(Result<Extracted, TextError>);

    impl TextSource for FixedText {
        fn extract(&self, _path: &Path) -> Result<Extracted, TextError> {
            self.0.clone()
        }
    }

    #[test]
    fn the_text_of_a_file_is_stored_in_the_index() {
        let inbox_dir = TempDir::new().unwrap();
        let inbox = location(&inbox_dir, LocationMode::Inbox);
        put(&inbox, "invoice.pdf", INVOICE);
        let fixture = IndexFixture::new();
        let chain = TextChain::new().with_source(FixedText(Ok(Extracted::Text(
            "Invoice 4711 Company A".to_owned(),
        ))));
        let runner = Runner::start(
            settings(workers(1)),
            fixture.steps_reading(chain),
            vec![inbox],
            stepping_clock(),
        );

        let report = runner.wait_for("the file reviewed", |snapshot| {
            is_idle(snapshot) && snapshot.steps[3].done == 1
        });

        assert!(report.errors.is_empty(), "{:?}", report.errors);
        let index = fixture.index.lock().unwrap();
        assert_eq!(index.search_text("4711 Company", 10).unwrap().len(), 1);
    }

    #[test]
    fn a_file_without_text_still_reaches_the_review_list() {
        let inbox_dir = TempDir::new().unwrap();
        let inbox = location(&inbox_dir, LocationMode::Inbox);
        put(&inbox, "invoice.pdf", INVOICE);
        let fixture = IndexFixture::new();
        let chain = TextChain::new().with_source(FixedText(Ok(Extracted::NotApplicable)));
        let runner = Runner::start(
            settings(workers(1)),
            fixture.steps_reading(chain),
            vec![inbox],
            stepping_clock(),
        );

        runner.wait_for("the file reviewed", |snapshot| {
            is_idle(snapshot) && snapshot.steps[3].done == 1
        });

        assert_eq!(
            fixture.index.lock().unwrap().open_reviews().unwrap().len(),
            1
        );
    }

    #[test]
    fn a_scan_without_the_ocr_models_goes_to_the_review_list_without_text() {
        let inbox_dir = TempDir::new().unwrap();
        let models_dir = TempDir::new().unwrap();
        let inbox = location(&inbox_dir, LocationMode::Inbox);
        put(&inbox, "scan.pdf", INVOICE);
        let fixture = IndexFixture::new();
        let page = PageImage {
            width: 2,
            height: 2,
            pixels: vec![0, 255, 255, 0],
        };
        let chain = TextChain::new()
            .with_source(FixedText(Ok(Extracted::PageImages(vec![page]))))
            .with_image_source(LazyOcrText::new(models_dir.path().join("ocr-models")));
        let runner = Runner::start(
            settings(workers(1)),
            fixture.steps_reading(chain),
            vec![inbox],
            stepping_clock(),
        );

        let report = runner.wait_for("the scan reviewed", |snapshot| {
            is_idle(snapshot) && snapshot.steps[3].done == 1
        });

        assert!(report.errors.is_empty(), "{:?}", report.errors);
        let index = fixture.index.lock().unwrap();
        let reviews = index.open_reviews().unwrap();
        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0].reason, ncrs_index::ReviewReason::NoMatchingRule);
        assert!(index.search_text("4711", 10).unwrap().is_empty());
        assert!(
            !models_dir.path().join("ocr-models").exists(),
            "nothing is created or downloaded"
        );
    }

    #[test]
    fn a_missing_pdf_library_holds_the_file_back_without_an_error_or_a_review() {
        let inbox_dir = TempDir::new().unwrap();
        let inbox = location(&inbox_dir, LocationMode::Inbox);
        put(&inbox, "invoice.pdf", INVOICE);
        let fixture = IndexFixture::new();
        let missing = TextError::LibraryMissing {
            path: PathBuf::from("pdfium"),
        };
        let chain = TextChain::new().with_source(FixedText(Err(missing)));
        let runner = Runner::start(
            settings(workers(1)),
            fixture.steps_reading(chain),
            vec![inbox],
            stepping_clock(),
        );

        let report = runner.wait_for("the file held back", |snapshot| {
            is_idle(snapshot) && snapshot.steps[1].unchanged == 1
        });

        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(report.steps[1].failed, 0);
        assert_eq!(report.steps[2].done, 0);
        assert_eq!(report.steps[3].done, 0);
        let index = fixture.index.lock().unwrap();
        assert!(index.open_reviews().unwrap().is_empty());
        let statuses = index.documents_with_status(DocumentStatus::New).unwrap();
        assert_eq!(statuses.len(), 1);
    }

    #[test]
    fn an_unreadable_file_is_still_an_error_of_the_text_step() {
        let inbox_dir = TempDir::new().unwrap();
        let inbox = location(&inbox_dir, LocationMode::Inbox);
        put(&inbox, "invoice.pdf", INVOICE);
        let fixture = IndexFixture::new();
        let broken = TextError::Unreadable {
            path: PathBuf::from("invoice.pdf"),
            reason: "broken".to_owned(),
        };
        let chain = TextChain::new().with_source(FixedText(Err(broken.clone())));
        let runner = Runner::start(
            settings(workers(1)),
            fixture.steps_reading(chain),
            vec![inbox],
            stepping_clock(),
        );

        let report = runner.wait_for("the failure shown", |snapshot| {
            is_idle(snapshot) && snapshot.steps[1].failed == 1
        });

        assert_eq!(report.errors[0].message, broken.to_string());
        assert!(fixture
            .index
            .lock()
            .unwrap()
            .open_reviews()
            .unwrap()
            .is_empty());
    }

    #[derive(Debug)]
    struct SwitchableText(Arc<Mutex<Result<Extracted, TextError>>>);

    impl TextSource for SwitchableText {
        fn extract(&self, _path: &Path) -> Result<Extracted, TextError> {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }
    }

    fn archive_scan(fixture: &IndexFixture, archive: &Location, chain: TextChain) -> Snapshot {
        let runner = Runner::start(
            settings(workers(1)),
            fixture.steps_reading(chain),
            vec![archive.clone()],
            stepping_clock(),
        );
        runner.wait_for("the archive scanned", is_idle)
    }

    #[test]
    fn an_archive_document_whose_text_could_not_be_read_gets_it_on_a_later_scan() {
        let archive_dir = TempDir::new().unwrap();
        let archive = location(&archive_dir, LocationMode::Archive);
        put(&archive, "invoice.pdf", INVOICE);
        let fixture = IndexFixture::new();
        let source = Arc::new(Mutex::new(Err(TextError::LibraryMissing {
            path: PathBuf::from("pdfium"),
        })));
        let chain = || TextChain::new().with_source(SwitchableText(Arc::clone(&source)));

        let first = archive_scan(&fixture, &archive, chain());
        assert!(first.errors.is_empty(), "{:?}", first.errors);
        assert!(fixture
            .index
            .lock()
            .unwrap()
            .search_text("4711", 10)
            .unwrap()
            .is_empty());

        *source.lock().unwrap() = Ok(Extracted::Text("Invoice 4711 Company A".to_owned()));
        archive_scan(&fixture, &archive, chain());

        let index = fixture.index.lock().unwrap();
        assert_eq!(index.search_text("4711", 10).unwrap().len(), 1);
    }

    #[test]
    fn an_archive_document_without_text_is_read_once_and_not_again() {
        let archive_dir = TempDir::new().unwrap();
        let archive = location(&archive_dir, LocationMode::Archive);
        put(&archive, "scan.pdf", INVOICE);
        let fixture = IndexFixture::new();
        let source = Arc::new(Mutex::new(Ok(Extracted::NotApplicable)));
        let chain = || TextChain::new().with_source(SwitchableText(Arc::clone(&source)));

        archive_scan(&fixture, &archive, chain());
        let id = fixture
            .index
            .lock()
            .unwrap()
            .documents_under(&archive.path)
            .unwrap()[0]
            .0;
        assert_eq!(
            fixture.index.lock().unwrap().document_text(id).unwrap(),
            Some(String::new())
        );

        *source.lock().unwrap() = Ok(Extracted::Text("Invoice 4711".to_owned()));
        let second = archive_scan(&fixture, &archive, chain());

        assert_eq!(second.steps[1].done, 0);
        let index = fixture.index.lock().unwrap();
        assert_eq!(index.document_text(id).unwrap(), Some(String::new()));
    }

    fn invoice_rule(filing_pattern: &str, confirmed: bool) -> ncrs_index::RuleSpec {
        ncrs_index::RuleSpec {
            match_text: Some(ncrs_index::TextMatch::Contains("company a".to_owned())),
            match_sender: None,
            extract_number: Some(r"Invoice (\d+)".to_owned()),
            extract_date: None,
            extract_amount: None,
            kind: "invoice".to_owned(),
            tax_context: ncrs_index::TaxContext::Company,
            tags: std::collections::BTreeSet::new(),
            filing_pattern: filing_pattern.to_owned(),
            confirmed,
        }
    }

    fn run_with_rule(spec: &ncrs_index::RuleSpec) -> IndexFixture {
        let inbox_dir = TempDir::new().unwrap();
        let inbox = location(&inbox_dir, LocationMode::Inbox);
        put(&inbox, "invoice.pdf", INVOICE);
        let fixture = IndexFixture::new();
        fixture.index.lock().unwrap().create_rule(spec).unwrap();
        let chain = TextChain::new().with_source(FixedText(Ok(Extracted::Text(
            "Invoice 4711 Company A".to_owned(),
        ))));
        let runner = Runner::start(
            settings(workers(1)),
            fixture.steps_reading(chain),
            vec![inbox],
            stepping_clock(),
        );
        let report = runner.wait_for("the file handled", |snapshot| {
            is_idle(snapshot) && snapshot.steps[2].done == 1
        });
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        fixture
    }

    #[test]
    fn a_document_matching_a_confirmed_rule_gets_its_fields_and_no_review() {
        let fixture = run_with_rule(&invoice_rule("{number}.pdf", true));

        let index = fixture.index.lock().unwrap();
        assert!(index.open_reviews().unwrap().is_empty());
        let documents = index.search_text("4711", 10).unwrap();
        let document = index.document(documents[0]).unwrap().unwrap();
        assert_eq!(document.fields.number.as_deref(), Some("4711"));
        assert_eq!(document.fields.kind.as_deref(), Some("invoice"));
        assert_eq!(document.status, DocumentStatus::Planned);
    }

    #[test]
    fn a_document_the_rules_cannot_place_stays_new_until_it_is_reviewed() {
        let fixture = run_with_rule(&invoice_rule("{number}.pdf", false));

        let index = fixture.index.lock().unwrap();
        let documents = index.search_text("4711", 10).unwrap();
        let document = index.document(documents[0]).unwrap().unwrap();
        assert_eq!(document.status, DocumentStatus::InReview);
    }

    #[test]
    fn a_document_with_a_missing_field_is_reviewed_with_that_field() {
        let fixture = run_with_rule(&invoice_rule("{year}/{number}.pdf", true));

        let index = fixture.index.lock().unwrap();
        let reviews = index.open_reviews().unwrap();
        assert_eq!(reviews.len(), 1);
        assert_eq!(
            reviews[0].reason,
            ncrs_index::ReviewReason::MissingField(ncrs_index::Field::Date)
        );
    }

    #[test]
    fn an_unconfirmed_rule_sends_the_document_to_review_as_such() {
        let fixture = run_with_rule(&invoice_rule("{number}.pdf", false));

        let index = fixture.index.lock().unwrap();
        let reviews = index.open_reviews().unwrap();
        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0].reason, ncrs_index::ReviewReason::UnconfirmedRule);
    }

    #[test]
    fn the_change_log_numbers_entries_and_tells_a_slow_consumer_it_missed_some() {
        let mut log = ChangeLog::default();
        for number in 0..MAX_CHANGED_PATHS + 3 {
            log.push(Path::new(&format!("/in/{number}.pdf")));
        }
        log.push(Path::new("/in/514.pdf"));
        let mut snapshot = Snapshot::off();
        snapshot.changed_paths = log.entries.iter().cloned().collect();
        snapshot.changed_sequence = log.latest;
        snapshot.changed_sequence_floor = log.floor;

        assert_eq!(snapshot.changed_paths.len(), MAX_CHANGED_PATHS);
        assert_eq!(snapshot.changed_sequence, (MAX_CHANGED_PATHS + 3) as u64);
        assert_eq!(snapshot.changed_sequence_floor, 3);
        assert!(snapshot.changes_since(2).is_none());
        let recent: Vec<_> = snapshot
            .changes_since(snapshot.changed_sequence - 2)
            .unwrap()
            .collect();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[1].path, Path::new("/in/514.pdf"));
        assert_eq!(
            snapshot.changes_since(3).unwrap().count(),
            MAX_CHANGED_PATHS
        );
        assert!(snapshot
            .changes_since(snapshot.changed_sequence + 1)
            .is_none());
    }

    #[test]
    fn files_that_changed_are_listed_and_files_found_unchanged_are_not() {
        let (_dir, location) = numbered_files(2);
        let (step, _started) = probe("probe", None, leave_the_second_file_unchanged);
        let runner = start(workers(1), vec![step], &location, Arc::new(FixedClock));

        let report = runner.wait_for("both files handled", |snapshot| {
            is_idle(snapshot) && snapshot.left_pipeline == 2
        });

        let paths: Vec<_> = report
            .changed_paths
            .iter()
            .map(|change| change.path.clone())
            .collect();
        assert_eq!(paths, vec![location.path.join("file-1.pdf")]);
        assert_eq!(report.changed_sequence, 1);
        assert_eq!(report.changed_sequence_floor, 0);
    }

    #[test]
    fn a_failed_file_is_listed_as_changed() {
        let (_dir, location) = numbered_files(3);
        let (step, _started) = probe("probe", None, fail_the_second_file);
        let runner = start(workers(1), vec![step], &location, Arc::new(FixedClock));

        let report = runner.wait_for("all handled", |snapshot| {
            is_idle(snapshot) && snapshot.left_pipeline == 3
        });

        assert!(report
            .changed_paths
            .iter()
            .any(|change| change.path == location.path.join("file-2.pdf")));
    }

    #[test]
    fn stopping_ends_the_workers_and_can_be_repeated() {
        let (_dir, location) = numbered_files(1);
        let (step, _started) = probe("probe", None, continue_always);
        let mut runner = start(workers(2), vec![step], &location, Arc::new(FixedClock));
        runner.wait_for("the file handled", |snapshot| {
            is_idle(snapshot) && snapshot.steps[0].done == 1
        });

        runner.stop();

        let report = runner.snapshot();
        assert_eq!(report.state, RunnerState::Off);
        assert!(report.workers.is_empty());
        runner.stop();
        drop(runner);
        assert_eq!(Snapshot::off().state, RunnerState::Off);
    }

    #[test]
    fn dropping_a_runner_right_after_the_start_does_not_hang() {
        let (_dir, location) = numbered_files(3);
        let (step, _started) = probe("probe", None, continue_always);
        drop(start(
            workers(2),
            vec![step],
            &location,
            Arc::new(FixedClock),
        ));
    }
}
