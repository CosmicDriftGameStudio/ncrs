use std::sync::{Mutex, MutexGuard, TryLockError};

use ncrs_index::Index;

/// For the UI thread: a pipeline or a run may hold the index for minutes, so
/// the UI never waits for it. `None` when the slot is busy.
pub fn try_lock(slot: &Mutex<Option<Index>>) -> Option<MutexGuard<'_, Option<Index>>> {
    match slot.try_lock() {
        Ok(guard) => Some(guard),
        Err(TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}
