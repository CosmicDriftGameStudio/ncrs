//! Removing entries: to the trash by default, for good on request.
//!
//! Nothing here is async; the caller runs it on the blocking pool, one entry
//! at a time, so the job can be stopped between two entries.

use std::io;
use std::path::Path;

/// Where "delete" puts an entry that can still be got back.
///
/// A trait so the end-to-end tests can hand the app a trash of their own. The
/// system trash is the developer's real one, and a test must never fill it.
pub trait Trash: Send + Sync {
    /// Moves `path` to the trash. The error is already text: the platform
    /// errors of the `trash` crate have no common type the app could match on.
    fn trash(&self, path: &Path) -> Result<(), String>;
}

/// The operating system's trash, through the `trash` crate.
pub struct SystemTrash;

impl Trash for SystemTrash {
    fn trash(&self, path: &Path) -> Result<(), String> {
        trash::delete(path).map_err(|err| err.to_string())
    }
}

/// Deletes `path` for good. A symlink is removed itself and never followed: a
/// link to a directory elsewhere must not take that directory along.
pub fn remove_permanently(path: &Path) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

// reason: a failing assertion is the signal in a test
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_and_a_tree_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "x").unwrap();
        let tree = dir.path().join("tree");
        std::fs::create_dir_all(tree.join("inner")).unwrap();
        std::fs::write(tree.join("inner/b.txt"), "y").unwrap();

        remove_permanently(&file).unwrap();
        remove_permanently(&tree).unwrap();

        assert!(!file.exists() && !tree.exists());
    }

    /// `symlink_metadata` is the guard: `metadata` would call the link a
    /// directory and `remove_dir_all` would then empty the target.
    #[cfg(unix)]
    #[test]
    fn a_link_to_a_directory_leaves_the_target_alone() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("keep.txt"), "k").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&outside, &link).unwrap();

        remove_permanently(&link).unwrap();

        assert!(std::fs::symlink_metadata(&link).is_err(), "link remains");
        assert!(outside.join("keep.txt").is_file(), "target was touched");
    }

    #[test]
    fn a_missing_entry_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(remove_permanently(&dir.path().join("gone")).is_err());
    }
}
