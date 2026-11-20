//! One running ncrs per data folder: the first start holds a lock on a file
//! there, a second start sees it taken and stays away from the data.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::Path;

const LOCK_FILE: &str = "ncrs.lock";

/// Held for as long as the value lives; the system lets go of it when the
/// process ends, however it ends.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
}

/// What taking the lock came to.
#[derive(Debug)]
pub enum Instance {
    /// No other ncrs holds the data folder.
    First(InstanceLock),
    /// Another ncrs holds it.
    Second,
    /// The lock file could not be made or locked, so nothing says an ncrs
    /// runs: the start goes on as the first, without a lock.
    Unlocked,
}

/// Takes the lock in `data_dir`, creating the folder when it is missing.
pub fn acquire(data_dir: &Path) -> Instance {
    if fs::create_dir_all(data_dir).is_err() {
        return Instance::Unlocked;
    }
    let file = match OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(data_dir.join(LOCK_FILE))
    {
        Ok(file) => file,
        Err(_) => return Instance::Unlocked,
    };
    match file.try_lock() {
        Ok(()) => Instance::First(InstanceLock { _file: file }),
        Err(TryLockError::WouldBlock) => Instance::Second,
        Err(TryLockError::Error(_)) => Instance::Unlocked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_second_start_in_one_folder_sees_the_lock_taken() {
        let scratch = tempfile::tempdir().expect("scratch folder");
        let first = acquire(scratch.path());
        assert!(matches!(first, Instance::First(_)));
        assert!(matches!(acquire(scratch.path()), Instance::Second));
    }

    #[test]
    fn the_lock_is_free_again_once_the_first_start_ends() {
        let scratch = tempfile::tempdir().expect("scratch folder");
        drop(acquire(scratch.path()));
        assert!(matches!(acquire(scratch.path()), Instance::First(_)));
    }
}
