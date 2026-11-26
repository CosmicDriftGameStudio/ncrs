//! What the UI shows while a run holds the index: the kinds last read. Every
//! handle of one module shares them, so a write through any of them drops what
//! the others would still show.

use std::sync::{Arc, Mutex, PoisonError};

use ncrs_index::Kinds;

#[derive(Clone, Debug, Default)]
pub struct Snapshots {
    kinds: Arc<Mutex<Option<Kinds>>>,
}

impl Snapshots {
    pub(crate) fn kinds(&self) -> Option<Kinds> {
        self.kinds
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn remember_kinds(&self, kinds: &Kinds) {
        *self.kinds.lock().unwrap_or_else(PoisonError::into_inner) = Some(kinds.clone());
    }

    /// Called after every write to rules or kinds: the next read while the
    /// index is busy has nothing to show rather than something outdated.
    pub(crate) fn forget(&self) {
        *self.kinds.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
}
