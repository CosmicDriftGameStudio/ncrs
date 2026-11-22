//! The calls the app makes on a running [`Runner`].

use super::*;

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
            priority: settings.load.thread_priority(),
            workers: Vec::new(),
            handles: Vec::new(),
            watcher: None,
            start_pending: true,
            scans: VecDeque::new(),
            first_step: VecDeque::new(),
            continuations: VecDeque::new(),
            waiting_first: HashSet::new(),
            continuous_roots: locations
                .iter()
                .filter(|location| location.continuous)
                .map(|location| location.path.clone())
                .collect(),
            taken: HashMap::new(),
            deferred: Vec::new(),
            gone: Default::default(),
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
            candidates: Candidates::default(),
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

    /// Whether the candidates were read from the index once for this runner.
    pub fn continuous_seeded(&self) -> bool {
        self.shared.lock().candidates.is_seeded()
    }

    /// Hands over the files the index holds as new and without review in
    /// the continuous locations, which the runner does not look at again
    /// after a restart.
    pub fn seed_continuous_candidates(&self, found: Vec<ContinuousCandidate>) {
        self.shared.update(|state| state.candidates.seed(found));
    }

    pub fn continuous_candidates_waiting(&self) -> usize {
        self.shared.lock().candidates.len()
    }

    /// Whether files the rules left open wait in a continuous location for
    /// which `wanted` says yes (given the location's root).
    pub fn has_continuous_candidates(&self, wanted: &dyn Fn(&Path) -> bool) -> bool {
        self.shared.lock().candidates.any_in(wanted)
    }

    /// Takes the waiting files of the continuous locations for which `wanted`
    /// says yes; the others keep waiting.
    pub fn take_continuous_candidates(
        &self,
        wanted: &dyn Fn(&Path) -> bool,
    ) -> Vec<ContinuousCandidate> {
        self.shared.update(|state| state.candidates.take_in(wanted))
    }

    /// The locations (by root) that file by themselves from now on; files
    /// that are already running keep what they started with.
    pub fn set_continuous_roots(&self, new_roots: impl IntoIterator<Item = PathBuf>) {
        let roots: HashSet<PathBuf> = new_roots.into_iter().collect();
        self.shared.update(|state| state.continuous_roots = roots);
    }

    /// Workers that are above the new target leave after their current job.
    pub fn set_load(&self, load: Load) {
        let target = load.worker_count(self.shared.settings.logical_cores).get();
        self.shared.update(|state| {
            state.target = target;
            state.priority = load.thread_priority();
        });
        self.shared.work_ready.notify_all();
        self.shared.ensure_workers();
    }

    /// Reads the runner's own state only, so it is safe on the UI thread while
    /// a worker holds the index.
    pub fn snapshot(&self) -> Snapshot {
        self.shared.snapshot()
    }

    /// Only the state of `snapshot`, without copying steps, workers and errors.
    pub fn state(&self) -> RunnerState {
        self.shared.runner_state()
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
    pub(super) fn wait_for(&self, what: &str, until: impl Fn(&Snapshot) -> bool) -> Snapshot {
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
