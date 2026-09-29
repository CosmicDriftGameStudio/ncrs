use std::io;
use std::path::{Path, PathBuf};

use super::FileEntry;

/// Why a directory read failed, without any user-facing text. The message is
/// formatted by the UI layer, which owns the language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// The OS refused to read the directory. The inner string is the OS message.
    CannotRead { reason: String },
    /// The background task itself failed, not the filesystem operation.
    TaskFailed { reason: String },
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::CannotRead { reason } | ReadError::TaskFailed { reason } => {
                f.write_str(reason)
            }
        }
    }
}

/// Result of reading a directory. Reading never "fails" from the caller's
/// perspective: on error, `entries` contains only `..` (if any) and `error`
/// holds a structured cause the UI renders in the current language.
#[derive(Debug, Clone)]
pub struct Listing {
    pub path: PathBuf,
    pub entries: Vec<FileEntry>,
    pub error: Option<ReadError>,
}

/// Reads `path` on tokio's blocking thread pool so the UI never blocks.
pub async fn read_directory(path: PathBuf) -> Listing {
    let target = path.clone();
    let result = tokio::task::spawn_blocking(move || read_sync(&target)).await;

    match result {
        Ok(Ok(entries)) => Listing {
            path,
            entries,
            error: None,
        },
        Ok(Err(err)) => Listing {
            entries: parent_only(&path),
            error: Some(ReadError::CannotRead {
                reason: err.to_string(),
            }),
            path,
        },
        Err(join_err) => Listing {
            entries: parent_only(&path),
            error: Some(ReadError::TaskFailed {
                reason: join_err.to_string(),
            }),
            path,
        },
    }
}

fn read_sync(path: &Path) -> io::Result<Vec<FileEntry>> {
    let mut entries: Vec<FileEntry> = std::fs::read_dir(path)?
        .filter_map(Result::ok)
        .map(|e| FileEntry::from_dir_entry(&e))
        .collect();

    if let Some(parent) = path.parent() {
        entries.push(FileEntry::parent(parent));
    }
    entries.sort_by(FileEntry::listing_order);
    Ok(entries)
}

fn parent_only(path: &Path) -> Vec<FileEntry> {
    path.parent().map(FileEntry::parent).into_iter().collect()
}

/// The user's home directory, falling back to the current dir or `/`.
pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Filesystem root of `path` (`/` on Unix, e.g. `C:\` on Windows).
///
/// Not used at startup any more — see [`start_dir`]. Kept because "go to the
/// filesystem root" is a standard file-manager action and will need it.
#[allow(dead_code)]
pub fn root_of(path: &Path) -> PathBuf {
    path.ancestors()
        .last()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// The directory the app should start in when the user has no better idea:
/// the working directory, falling back to the home directory.
pub fn start_dir() -> PathBuf {
    std::env::current_dir()
        .ok()
        .filter(|p| p.is_dir())
        .unwrap_or_else(home_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One scratch directory per test, so parallel `#[tokio::test]`s never share
    /// paths. `label` must be a filename-safe, per-test-unique string.
    fn scratch_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ncrs-{label}-{}", std::process::id()))
    }

    #[tokio::test]
    async fn reads_directory_with_parent_first() {
        let dir = scratch_dir("read-listing");
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("b.txt"), b"hello").unwrap();
        std::fs::write(dir.join("A.txt"), b"").unwrap();

        let listing = read_directory(dir.clone()).await;
        let names: Vec<_> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["..", "sub", "A.txt", "b.txt"]);
        assert!(listing.error.is_none());
        assert_eq!(listing.entries[3].size, 5);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn missing_directory_yields_error_not_panic() {
        let listing = read_directory(PathBuf::from("/definitely/not/here")).await;
        assert!(listing.error.is_some());
        assert_eq!(listing.entries.len(), 1);
        assert!(listing.entries[0].is_parent);
    }
}

#[cfg(test)]
mod start_dir_tests {
    use super::*;

    /// Regression: the app opened its right panel at the filesystem root
    /// instead of a useful directory, because `root_of` was used where the
    /// working directory was meant.
    #[test]
    fn start_dir_is_the_working_directory() {
        let start = start_dir();
        assert!(
            start.is_dir(),
            "start_dir {:?} is not a directory",
            start.display()
        );
        // Falls back to home rather than the filesystem root.
        assert_ne!(
            start.parent(),
            None,
            "start_dir should not be the filesystem root"
        );
    }

    #[test]
    fn root_of_finds_the_filesystem_root() {
        let root = root_of(Path::new("/a/b/c"));
        assert_eq!(root, PathBuf::from("/"));
    }

    /// `start_dir` must never hand out a path that cannot be listed, so a
    /// fresh panel shows entries rather than an error.
    #[tokio::test]
    async fn start_dir_is_readable() {
        let listing = read_directory(start_dir()).await;
        assert!(
            listing.error.is_none(),
            "start_dir could not be read: {:?}",
            listing.error
        );
    }
}
