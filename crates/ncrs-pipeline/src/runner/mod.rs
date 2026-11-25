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
use ncrs_priority::{set_current_thread_priority, ThreadPriority};

use crate::clock::Clock;
use crate::load::Load;
use crate::step::{Item, Step, StepOutcome};

mod gone;
mod handle;
mod snapshot;
mod state;
mod worker;
use gone::GoneQueue;
use handle::forward_watch_events;
use snapshot::ChangeLog;
pub use snapshot::{
    Busy, ChangeOutcome, ChangedPath, ErrorEntry, RunnerState, Settings, Snapshot, StepSnapshot,
    Task, WorkerSnapshot, MAX_CHANGED_PATHS, MAX_ERRORS, RATE_MINUTES,
};
use worker::worker_main;

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
    Gone {
        path: PathBuf,
        location: Location,
    },
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
    /// What the workers run at; each applies a change before its next job.
    priority: ThreadPriority,
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
    /// Items whose next step `waits_for_the_rest`, without their text. Served
    /// only when nothing else is left to do.
    late: VecDeque<Continuation>,
    /// Per step: `Step::waits_for_the_rest`.
    waits_for_the_rest: Vec<bool>,
    /// Each path as it was when it last left the first-step queue since the
    /// latest scan of its location started. Recorded at that point, so a
    /// report arriving while the file runs is matched too.
    taken: HashMap<PathBuf, Fingerprint>,
    deferred: Vec<Deferred>,
    gone: GoneQueue,
    next_rescan: Option<Instant>,
    batches: HashMap<u64, Batch>,
    next_batch: u64,
    counts: Vec<StepCounts>,
    rate: VecDeque<SystemTime>,
    errors: VecDeque<ErrorEntry>,
    left_pipeline: usize,
    changes: ChangeLog,
}

struct Shared {
    state: Mutex<State>,
    /// Signalled when there may be work or a worker has to exit.
    work_ready: Condvar,
    /// Signalled on every change of the state.
    changed: Condvar,
    steps: Vec<Box<dyn Step>>,
    locations: Vec<Location>,
    /// Locations that came after the start, see `Runner::add_location`.
    added_locations: Mutex<Vec<Location>>,
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

    fn runner_state(&self) -> RunnerState {
        Self::runner_state_of(&self.lock())
    }

    fn runner_state_of(state: &State) -> RunnerState {
        if state.stopping {
            RunnerState::Off
        } else if state.paused {
            RunnerState::Paused
        } else if state.has_queued_work() || state.workers.iter().any(|slot| slot.busy.is_some()) {
            RunnerState::Running
        } else {
            RunnerState::Idle
        }
    }

    fn snapshot_of(&self, state: &State) -> Snapshot {
        let now = self.clock.now();
        let runner_state = Self::runner_state_of(state);
        let mut waiting: Vec<usize> = vec![0; self.steps.len()];
        if let Some(first) = waiting.first_mut() {
            *first = state.first_step.len();
        }
        for step in state
            .continuations
            .iter()
            .map(|queued| queued.step)
            .chain(state.late.iter().map(|queued| queued.step))
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
            changed_paths: state.changes.entries.iter().cloned().collect(),
            changed_sequence: state.changes.latest,
            changed_sequence_floor: state.changes.floor,
        }
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

#[cfg(test)]
mod tests;

/// Per-step timing on stderr, for measuring where a file spends its time.
fn timing_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("NCRS_PIPELINE_TIMING").is_some())
}
