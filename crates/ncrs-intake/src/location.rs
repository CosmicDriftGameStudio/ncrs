use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::IntakeError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LocationMode {
    /// Documents are taken in and later filed away.
    Inbox,
    /// Indexed where they are. Read-only.
    Archive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    /// Canonical, see `Location::new`.
    pub path: PathBuf,
    pub mode: LocationMode,
    pub images: bool,
}

impl Location {
    /// The path is canonicalised and must be an existing directory. Walk and
    /// watcher both work on this root: on macOS FSEvents reports
    /// `/private/var/...` for a directory that is spelled `/var/...`, and two
    /// spellings of one file would be recorded as a fake copy.
    pub fn new(path: &Path, mode: LocationMode, images: bool) -> Result<Self, IntakeError> {
        let canonical = fs::canonicalize(path).map_err(IntakeError::io(path))?;
        if !canonical.is_dir() {
            return Err(IntakeError::io(path)(io::Error::new(
                io::ErrorKind::NotADirectory,
                "not a directory",
            )));
        }
        Ok(Self {
            path: canonical,
            mode,
            images,
        })
    }
}
