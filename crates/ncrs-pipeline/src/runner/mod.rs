use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::fs;
use std::ops::ControlFlow;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use ncrs_intake::{classify, walk_location, Found, Location, LocationWatcher, WatchEvent};

use crate::clock::Clock;
use crate::load::Load;
use crate::step::{Item, Step, StepOutcome};

mod snapshot;
use snapshot::ChangeLog;
pub use snapshot::{
    Busy, ChangeOutcome, ChangedPath, ErrorEntry, RunnerState, Settings, Snapshot, StepSnapshot,
    Task, WorkerSnapshot, MAX_CHANGED_PATHS, MAX_ERRORS, RATE_MINUTES,
};

const PANIC_MESSAGE: &str = "the step stopped unexpectedly";
/// Never shown in practice: items only ever name a registered step.
const UNKNOWN_STEP: &str = "unknown";

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
    /// A replaced file keeps size and mtime when copied with `cp -p` or
    /// unpacked from an archive, but gets a new inode or change time.
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}

impl Fingerprint {
    fn of(path: &Path) -> Option<Self> {
        let metadata = fs::metadata(path).ok()?;
        Some(Self {
            len: metadata.len(),
            modified: metadata.modified().ok()?,
            #[cfg(unix)]
            identity: {
                use std::os::unix::fs::MetadataExt as _;
                (
                    metadata.dev(),
                    metadata.ino(),
                    metadata.ctime(),
                    metadata.ctime_nsec(),
                )
            },
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
            sequence: self.changes.latest,
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
            // The worker will read the file as it is by then, so a repeat of
            // that state must be recognised.
            if let Some(queued) = self
                .first_step
                .iter_mut()
                .find(|queued| queued.item.path == path)
            {
                queued.fingerprint = fingerprint.or(queued.fingerprint);
            }
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
                force: false,
                stored_text_is_current: false,
            },
            batch,
            fingerprint,
        });
        true
    }

    fn queue_again(&mut self, location: &Location, path: &Path) {
        if !self.waiting_first.insert(path.to_path_buf()) {
            if let Some(queued) = self
                .first_step
                .iter_mut()
                .find(|queued| queued.item.path == path)
            {
                queued.item.force = true;
            }
            return;
        }
        self.first_step.push_back(Queued {
            item: Item {
                location: location.clone(),
                path: path.to_path_buf(),
                document: None,
                text: None,
                review: None,
                plan: None,
                force: true,
                stored_text_is_current: false,
            },
            batch: None,
            fingerprint: None,
        });
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
            .is_some_and(|&oldest| at.duration_since(oldest).map_or(true, |age| age >= window))
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
                let fingerprint = Fingerprint::of(&due_entry.item.path);
                self.first_step.push_back(Queued {
                    item: due_entry.item,
                    batch: None,
                    fingerprint,
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
                if fingerprint.is_none() {
                    // Deleted or moved away: nothing left to tell a repeat from.
                    self.update(|state| state.taken.remove(&path));
                }
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
                Ok(Ok(unsettled)) => {
                    for path in unsettled {
                        self.record_error(
                            &path,
                            Task::Step(step.name()),
                            "cannot be checked".to_owned(),
                        );
                    }
                    continue;
                }
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
                Found::Unreadable { path, message } => {
                    self.record_error(path, Task::Scan, message.to_owned());
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
                    let change = if last_step {
                        ChangeOutcome::Finished
                    } else {
                        ChangeOutcome::Advanced
                    };
                    self.changes.push(&item.path, change);
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
                self.changes.push(&item.path, ChangeOutcome::Finished);
                self.record_document(at);
            }
            Ok(StepOutcome::Unchanged) => {
                counts.unchanged += 1;
                self.left_pipeline += 1;
                if item.force {
                    self.changes.push(&item.path, ChangeOutcome::Unchanged);
                }
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
                self.changes.push(&item.path, ChangeOutcome::Failed);
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

    /// Runs a known file through all steps again, even if nothing about it
    /// changed. False when the path is in none of the locations.
    pub fn process_again(&self, path: &Path) -> bool {
        let Some(location) = self
            .shared
            .locations
            .iter()
            .find(|location| path.starts_with(&location.path))
        else {
            return false;
        };
        self.shared
            .update(|state| state.queue_again(location, path));
        self.shared.work_ready.notify_all();
        true
    }

    /// Scans the location that holds `path` now, through the normal first
    /// step: a file or folder that was left out until now is found without
    /// waiting for the next rescan. False when the path is in no location.
    pub fn rescan_around(&self, path: &Path) -> bool {
        let Some(location) = self
            .shared
            .locations
            .iter()
            .find(|location| path.starts_with(&location.path))
        else {
            return false;
        };
        self.shared.update(|state| state.queue_scan(location));
        self.shared.work_ready.notify_all();
        true
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
mod tests;
