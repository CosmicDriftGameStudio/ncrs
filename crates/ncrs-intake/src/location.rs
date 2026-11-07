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

/// The path as the rules see it: relative to the innermost location root it
/// lies under, so folders above the root take no part in a folder condition.
/// A path under no root stays as it is.
pub fn relative_to_location<'a>(path: &'a Path, roots: &[PathBuf]) -> &'a Path {
    roots
        .iter()
        .filter_map(|root| path.strip_prefix(root).ok())
        .min_by_key(|relative| relative.components().count())
        .unwrap_or(path)
}

/// The innermost location root the path lies under.
pub fn location_of<'a>(path: &Path, roots: &'a [PathBuf]) -> Option<&'a Path> {
    roots
        .iter()
        .filter(|root| path.starts_with(root))
        .max_by_key(|root| root.components().count())
        .map(PathBuf::as_path)
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

#[cfg(test)]
mod relative_tests {
    use super::*;

    #[test]
    fn the_part_above_the_innermost_root_is_dropped() {
        let roots = [PathBuf::from("/a/Steuer"), PathBuf::from("/a/Steuer/Inbox")];
        let path = Path::new("/a/Steuer/Inbox/new/x.pdf");
        assert_eq!(relative_to_location(path, &roots), Path::new("new/x.pdf"));
        assert_eq!(relative_to_location(path, &[]), path);
    }
}
