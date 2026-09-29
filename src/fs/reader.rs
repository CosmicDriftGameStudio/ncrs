use std::io;
use std::path::{Path, PathBuf};

use super::FileEntry;

/// Result of reading a directory. Reading never "fails" from the caller's
/// perspective: on error, `entries` contains only `..` (if any) and `error`
/// holds a human readable message.
#[derive(Debug, Clone)]
pub struct Listing {
    pub path: PathBuf,
    pub entries: Vec<FileEntry>,
    pub error: Option<String>,
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
            error: Some(format!("Cannot read {}: {err}", path.display())),
            path,
        },
        Err(join_err) => Listing {
            entries: parent_only(&path),
            error: Some(format!("Directory read task failed: {join_err}")),
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
pub fn root_of(path: &Path) -> PathBuf {
    path.ancestors()
        .last()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reads_directory_with_parent_first() {
        let dir = std::env::temp_dir().join(format!("ncrs-test-{}", std::process::id()));
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
