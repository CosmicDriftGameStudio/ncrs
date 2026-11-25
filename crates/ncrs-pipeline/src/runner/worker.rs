use super::*;

impl Shared {
    /// Blocks until there is a job for the worker; `None` when it has to exit.
    /// A worker whose id is at or above the target leaves here, so a job that
    /// is running is never cut short.
    pub(super) fn next_job_for(&self, id: usize) -> Option<Job> {
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
                        for location in &self.all_locations() {
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

    pub(super) fn busy_for(&self, job: &Job) -> Busy {
        let (file, task) = match job {
            Job::Start => (PathBuf::new(), Task::Watch),
            Job::Scan(location) => (location.path.clone(), Task::Scan),
            Job::Gone { path, .. } => (path.clone(), Task::Scan),
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

    pub(super) fn finish_job(&self, id: usize) {
        self.update(|state| {
            if let Some(slot) = state.workers.get_mut(id) {
                slot.busy = None;
            }
        });
    }

    pub(super) fn set_busy(&self, id: usize, file: &Path, task: Task) {
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

    pub(super) fn record_error(&self, file: &Path, task: Task, message: String) {
        let at = self.clock.now();
        self.update(|state| state.push_error(file.to_path_buf(), task, message, at));
    }

    pub(super) fn on_watch_event(&self, event: WatchEvent) {
        match event {
            WatchEvent::File(path) => {
                let Some(location) = self.location_of(&path) else {
                    return;
                };
                let fingerprint = Fingerprint::of(&path);
                self.update(|state| state.gone.forget(&path));
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
                        && state.queue_arrival(&location, &path, fingerprint)
                });
                if queued {
                    self.work_ready.notify_one();
                }
            }
            WatchEvent::Gone(path) => {
                let Some(location) = self.location_of(&path) else {
                    return;
                };
                let due = Instant::now().checked_add(self.settings.gone_settle_after);
                let queued = self.update(|state| {
                    state.taken.remove(&path);
                    if state.stopping {
                        return false;
                    }
                    state
                        .gone
                        .add(path, &location, due.unwrap_or_else(Instant::now));
                    true
                });
                if queued {
                    self.work_ready.notify_one();
                }
            }
            WatchEvent::Error { location, message } => {
                let at = self.clock.now();
                let affected_location = self.location_of(&location);
                self.update(|state| {
                    if state.stopping {
                        return;
                    }
                    state.push_error(location.clone(), Task::Watch, message, at);
                    if let Some(found) = affected_location {
                        state.queue_scan(&found);
                    }
                });
                self.work_ready.notify_one();
            }
        }
    }

    pub(super) fn run_after_scan(&self, id: usize, location: &Location) {
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

    pub(super) fn run_scan(&self, id: usize, location: &Location) {
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

    pub(super) fn run_item(
        &self,
        id: usize,
        mut item: Item,
        step_index: usize,
        batch: Option<u64>,
    ) {
        let name = self.step_name(step_index);
        let outcome = match self.steps.get(step_index) {
            Some(step) => {
                let now = self.clock.now();
                let started = Instant::now();
                let outcome = catch_unwind(AssertUnwindSafe(|| step.run(&mut item, now)))
                    .unwrap_or_else(|_panic| Err(PANIC_MESSAGE.to_owned()));
                if timing_enabled() {
                    eprintln!(
                        "[timing] {name} {} {}",
                        started.elapsed().as_millis(),
                        item.path.file_name().map_or_else(
                            || item.path.display().to_string(),
                            |file| file.to_string_lossy().into_owned(),
                        ),
                    );
                }
                outcome
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

pub fn run_start(shared: &Arc<Shared>) {
    let locations = shared.all_locations();
    let (watcher, failed) =
        LocationWatcher::start_forwarding(&locations, forward_watch_events(shared));
    let at = shared.clock.now();
    let unused_watcher = shared.update(|state| {
        for (location, err) in failed {
            state.push_error(location, Task::Watch, err.to_string(), at);
        }
        if state.stopping {
            return Some(watcher);
        }
        state.watcher = Some(watcher);
        // A location added while the watcher was being built was not in it.
        for location in shared.all_locations() {
            if !locations.contains(&location) {
                state.watch_added(&location, forward_watch_events(shared), at);
            }
            state.queue_scan(&location);
        }
        None
    });
    // Dropping a watcher joins its thread, which may be waiting for the lock.
    drop(unused_watcher);
    shared.work_ready.notify_all();
}

pub fn worker_main(shared: &Arc<Shared>, id: usize, epoch: u64) {
    let _exit = ExitGuard { shared, id, epoch };
    let mut applied = shared.lock().priority;
    set_current_thread_priority(applied);
    while let Some(job) = shared.next_job_for(id) {
        let wanted = shared.lock().priority;
        if wanted != applied {
            set_current_thread_priority(wanted);
            applied = wanted;
        }
        match job {
            Job::Start => run_start(shared),
            Job::Scan(location) => shared.run_scan(id, &location),
            Job::Gone { path, location } => shared.run_gone(&path, &location),
            Job::Item { item, step, batch } => shared.run_item(id, item, step, batch),
        }
        shared.finish_job(id);
    }
}
