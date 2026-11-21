//! Paths the watcher reported as deleted or moved out, settled one by one
//! after a short delay.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Instant;

use ncrs_intake::Location;

use super::{Shared, Task, PANIC_MESSAGE};
use std::panic::{catch_unwind, AssertUnwindSafe};

#[derive(Debug, Default)]
pub(super) struct GoneQueue {
    /// A path reported again keeps its first due time, so a burst of events
    /// for one path does not push the settling back.
    waiting: HashMap<PathBuf, (Instant, Location)>,
    ready: VecDeque<(PathBuf, Location)>,
}

impl GoneQueue {
    pub(super) fn add(&mut self, path: PathBuf, location: &Location, due: Instant) {
        self.waiting
            .entry(path)
            .or_insert_with(|| (due, location.clone()));
    }

    /// The path is there again; whatever the index holds for it is right.
    pub(super) fn forget(&mut self, path: &Path) {
        self.waiting.remove(path);
        self.ready.retain(|(ready_path, _)| ready_path != path);
    }

    pub(super) fn promote_due(&mut self, now: Instant) {
        let due: Vec<PathBuf> = self
            .waiting
            .iter()
            .filter(|(_, (when, _))| *when <= now)
            .map(|(path, _)| path.clone())
            .collect();
        for path in due {
            if let Some((_, location)) = self.waiting.remove(&path) {
                self.ready.push_back((path, location));
            }
        }
    }

    pub(super) fn pop_ready(&mut self) -> Option<(PathBuf, Location)> {
        self.ready.pop_front()
    }

    pub(super) fn earliest_due(&self) -> Option<Instant> {
        self.waiting.values().map(|(due, _)| *due).min()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.waiting.is_empty() && self.ready.is_empty()
    }
}

impl Shared {
    /// Settles the index at a path that is gone. A path that exists again, or
    /// that the app's own filing has repointed the document away from, is left
    /// alone by the step.
    pub(super) fn run_gone(&self, path: &Path, location: &Location) {
        for step in &self.steps {
            if self.is_stopping() {
                return;
            }
            let failed = match catch_unwind(AssertUnwindSafe(|| step.path_gone(location, path))) {
                Ok(Ok(unsettled)) => {
                    for unsettled_path in unsettled {
                        self.record_error(
                            &unsettled_path,
                            Task::Step(step.name()),
                            "cannot be checked".to_owned(),
                        );
                    }
                    continue;
                }
                Ok(Err(message)) => message,
                Err(_panic) => PANIC_MESSAGE.to_owned(),
            };
            self.record_error(path, Task::Step(step.name()), failed);
        }
    }
}
