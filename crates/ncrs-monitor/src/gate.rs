//! "Already running": one flag per kind of measurement, so a slow one is not
//! started again by the next tick.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct Gate(Arc<AtomicBool>);

impl Gate {
    /// The flag is set until the returned guard is dropped; `None` while a
    /// guard of this gate is alive.
    pub fn enter(&self) -> Option<GateGuard> {
        self.0
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            .then(|| GateGuard(Arc::clone(&self.0)))
    }

    #[cfg(test)]
    pub fn is_running(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Debug)]
pub struct GateGuard(Arc<AtomicBool>);

impl Drop for GateGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gate_stays_shut_until_the_guard_is_dropped() {
        let gate = Gate::default();
        let guard = gate.enter();
        assert!(guard.is_some());
        assert!(gate.enter().is_none());
        drop(guard);
        assert!(gate.enter().is_some());
    }
}
