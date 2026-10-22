use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// How long size and mtime must stay the same before a file counts as done.
pub const STABLE_AFTER: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy)]
struct Observation {
    size: u64,
    mtime: SystemTime,
    since: SystemTime,
}

/// Holds back files that are still being written or downloaded. The clock is
/// never read here; the caller passes `now`.
#[derive(Debug, Default)]
pub struct StabilityTracker {
    seen: HashMap<PathBuf, Observation>,
}

impl StabilityTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// True once an earlier observation of this path with the same size and
    /// mtime is at least `STABLE_AFTER` older than `now`; the path is then
    /// forgotten. Otherwise the observation is kept and the result is false.
    ///
    /// The mtime alone is not enough: Finder and `cp -p` give the copy the
    /// source's old mtime while it is still growing, so the size has to hold
    /// still too.
    pub fn observe(&mut self, path: &Path, size: u64, mtime: SystemTime, now: SystemTime) -> bool {
        match self.seen.get(path) {
            Some(earlier) if earlier.size == size && earlier.mtime == mtime => {
                let held = now.duration_since(earlier.since).unwrap_or_default();
                if held >= STABLE_AFTER {
                    self.seen.remove(path);
                    return true;
                }
            }
            _ => {
                self.seen.insert(
                    path.to_path_buf(),
                    Observation {
                        size,
                        mtime,
                        since: now,
                    },
                );
            }
        }
        false
    }

    /// Stops waiting for a path, for example because the file is gone.
    pub fn forget(&mut self, path: &Path) {
        self.seen.remove(path);
    }

    /// Paths seen but not yet stable, sorted, for the caller to look at again.
    pub fn pending(&self) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = self.seen.keys().cloned().collect();
        paths.sort();
        paths
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
    use std::time::UNIX_EPOCH;

    use super::*;

    const FILE: &str = "inbox/a.pdf";

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_000_000 + seconds)
    }

    #[test]
    fn the_first_observation_is_never_stable() {
        let mut tracker = StabilityTracker::new();

        assert!(!tracker.observe(Path::new(FILE), 10, at(0), at(0)));
        assert_eq!(tracker.pending(), vec![PathBuf::from(FILE)]);
    }

    #[test]
    fn the_same_size_and_mtime_for_long_enough_is_stable_and_forgotten() {
        let mut tracker = StabilityTracker::new();
        tracker.observe(Path::new(FILE), 10, at(0), at(0));

        assert!(!tracker.observe(Path::new(FILE), 10, at(0), at(1)));
        assert!(tracker.observe(Path::new(FILE), 10, at(0), at(0) + STABLE_AFTER));
        assert!(tracker.pending().is_empty());
    }

    #[test]
    fn a_growing_size_starts_the_wait_again() {
        let mut tracker = StabilityTracker::new();
        tracker.observe(Path::new(FILE), 10, at(0), at(0));

        assert!(!tracker.observe(Path::new(FILE), 20, at(0), at(1)));
        assert!(!tracker.observe(Path::new(FILE), 20, at(0), at(2)));
        assert!(tracker.observe(Path::new(FILE), 20, at(0), at(3)));
    }

    #[test]
    fn an_old_mtime_does_not_make_a_growing_copy_stable() {
        let mut tracker = StabilityTracker::new();
        let preserved_mtime = at(0);
        tracker.observe(Path::new(FILE), 10, preserved_mtime, at(100));

        assert!(!tracker.observe(Path::new(FILE), 20, preserved_mtime, at(110)));
        assert!(!tracker.observe(Path::new(FILE), 30, preserved_mtime, at(120)));
    }

    #[test]
    fn a_changed_mtime_starts_the_wait_again() {
        let mut tracker = StabilityTracker::new();
        tracker.observe(Path::new(FILE), 10, at(0), at(0));

        assert!(!tracker.observe(Path::new(FILE), 10, at(5), at(10)));
    }

    #[test]
    fn forgetting_a_path_removes_it_from_pending() {
        let mut tracker = StabilityTracker::new();
        tracker.observe(Path::new(FILE), 10, at(0), at(0));

        tracker.forget(Path::new(FILE));

        assert!(tracker.pending().is_empty());
    }

    #[test]
    fn an_observation_from_the_future_is_not_stable() {
        let mut tracker = StabilityTracker::new();
        tracker.observe(Path::new(FILE), 10, at(0), at(50));

        assert!(!tracker.observe(Path::new(FILE), 10, at(0), at(0)));
    }
}
