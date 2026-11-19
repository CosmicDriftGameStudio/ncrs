//! Counts the work that holds the index across several steps, so the app can
//! tell that starting over or encrypting would land in the middle of it.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct IndexWork(Arc<AtomicUsize>);

/// Work counts as running until this is dropped.
#[derive(Debug)]
#[must_use]
pub struct IndexWorkGuard(Arc<AtomicUsize>);

impl IndexWork {
    pub fn begin(&self) -> IndexWorkGuard {
        self.0.fetch_add(1, Ordering::SeqCst);
        IndexWorkGuard(Arc::clone(&self.0))
    }

    pub fn running(&self) -> bool {
        self.0.load(Ordering::SeqCst) > 0
    }
}

impl Drop for IndexWorkGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_runs_until_every_guard_is_dropped() {
        let work = IndexWork::default();
        assert!(!work.running());
        let first = work.begin();
        let second = work.clone().begin();
        drop(first);
        assert!(work.running());
        drop(second);
        assert!(!work.running());
    }
}
