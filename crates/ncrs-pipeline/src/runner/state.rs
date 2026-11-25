use super::*;

impl State {
    pub(super) fn push_error(&mut self, file: PathBuf, task: Task, message: String, at: SystemTime) {
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

    pub(super) fn queue_scan(&mut self, location: &Location) {
        if !self.scans.iter().any(|queued| queued.path == location.path) {
            self.scans.push_back(location.clone());
        }
    }

    /// False when the path already waits at the first step, which then covers
    /// the change.
    pub(super) fn queue_first_step(
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
            item: Item::at_first_step(location, path, false),
            batch,
            fingerprint,
        });
        true
    }

    /// A file that just arrived goes before the stock a scan found, so a slow
    /// step over a whole archive does not hold it back.
    pub(super) fn queue_arrival(
        &mut self,
        location: &Location,
        path: &Path,
        fingerprint: Option<Fingerprint>,
    ) -> bool {
        let queued = self.queue_first_step(location, path, None, fingerprint);
        if queued {
            if let Some(arrival) = self.first_step.pop_back() {
                self.first_step.push_front(arrival);
            }
        }
        queued
    }

    pub(super) fn queue_again(&mut self, location: &Location, path: &Path) {
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
            item: Item::at_first_step(location, path, true),
            batch: None,
            fingerprint: None,
        });
    }

    pub(super) fn is_repeated_report(&self, path: &Path, fingerprint: Option<Fingerprint>) -> bool {
        fingerprint.is_some_and(|reported| self.taken.get(path) == Some(&reported))
    }

    pub(super) fn release(&mut self, batch: u64) -> Option<Location> {
        let entry = self.batches.get_mut(&batch)?;
        entry.outstanding = entry.outstanding.saturating_sub(1);
        if entry.outstanding > 0 {
            return None;
        }
        let finished = self.batches.remove(&batch)?;
        (finished.settle && !self.stopping).then_some(finished.location)
    }

    pub(super) fn record_document(&mut self, at: SystemTime) {
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

    pub(super) fn promote_due(&mut self, now: Instant) {
        self.gone.promote_due(now);
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
                self.queue_continuation(due_entry.item, due_entry.step);
            }
        }
    }

    fn enriches(&self, step: usize) -> bool {
        self.waits_for_the_rest.get(step).copied().unwrap_or(false)
    }

    /// An item for a step that waits for the rest drops its text: thousands
    /// of them would otherwise sit in memory, and the step loads it again.
    pub(super) fn queue_continuation(&mut self, mut item: Item, step: usize) {
        if self.enriches(step) {
            item.text = None;
            self.late.push_back(Continuation { item, step });
        } else {
            self.continuations.push_back(Continuation { item, step });
        }
    }

    pub(super) fn next_job(&mut self) -> Option<Job> {
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
        if let Some((path, location)) = self.gone.pop_ready() {
            return Some(Job::Gone { path, location });
        }
        let Some(next) = self.first_step.pop_front() else {
            return self.pop_late();
        };
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

    /// Only when everything else, including what waits for a retry or for a
    /// path to settle, is done.
    fn pop_late(&mut self) -> Option<Job> {
        if !self.deferred.is_empty() || !self.gone.is_empty() {
            return None;
        }
        let next = self.late.pop_front()?;
        Some(Job::Item {
            item: next.item,
            step: next.step,
            batch: None,
        })
    }

    pub(super) fn earliest_wakeup(&self) -> Option<Instant> {
        let rescan = if self.paused { None } else { self.next_rescan };
        self.deferred
            .iter()
            .map(|entry| entry.due)
            .chain(rescan)
            .chain(self.gone.earliest_due())
            .min()
    }

    pub(super) fn has_queued_work(&self) -> bool {
        self.start_pending
            || !self.scans.is_empty()
            || !self.first_step.is_empty()
            || !self.continuations.is_empty()
            || !self.late.is_empty()
            || !self.deferred.is_empty()
            || !self.gone.is_empty()
    }
}

impl State {
    pub(super) fn apply(
        &mut self,
        item: Item,
        step_index: usize,
        name: &'static str,
        outcome: Result<StepOutcome, String>,
        at: SystemTime,
        retry_due: Option<Instant>,
    ) {
        let last_step = step_index + 1 >= self.counts.len();
        // A step that waits for the rest only enriches: the file already
        // left the pipeline, was counted and announced before it.
        let enriching = self.enriches(step_index);
        let next_enriches = self.enriches(step_index + 1);
        let Some(counts) = self.counts.get_mut(step_index) else {
            return;
        };
        match outcome {
            Ok(StepOutcome::Continue) => {
                counts.done += 1;
                if last_step || next_enriches {
                    self.changes.push(
                        &item.path,
                        if enriching {
                            ChangeOutcome::FilledByAi
                        } else {
                            ChangeOutcome::Finished
                        },
                    );
                    if !enriching {
                        self.left_pipeline += 1;
                        self.record_document(at);
                    }
                } else if step_index == 0 {
                    self.changes.push(&item.path, ChangeOutcome::Advanced);
                }
                if !last_step {
                    self.queue_continuation(item, step_index + 1);
                }
            }
            Ok(StepOutcome::Finished) => {
                counts.done += 1;
                if enriching {
                    self.changes.push(&item.path, ChangeOutcome::FilledByAi);
                } else {
                    self.left_pipeline += 1;
                    self.changes.push(&item.path, ChangeOutcome::Finished);
                    self.record_document(at);
                }
            }
            Ok(StepOutcome::Unchanged) => {
                counts.unchanged += 1;
                if !enriching {
                    self.left_pipeline += 1;
                    if item.force {
                        self.changes.push(&item.path, ChangeOutcome::Unchanged);
                    }
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
                if !enriching {
                    self.left_pipeline += 1;
                }
                self.changes.push(&item.path, ChangeOutcome::Failed);
                self.push_error(item.path, Task::Step(name), message, at);
            }
        }
    }
}
