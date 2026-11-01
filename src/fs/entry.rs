use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::FolderSize;

/// A single row in a directory listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// The name as the filesystem spells it. `OsString`, not `String`: a name
    /// that is not valid UTF-8 would become U+FFFD here, and two different
    /// files could then share one name — which matters once a tag is a name
    /// rather than a position.
    pub name: std::ffi::OsString,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
    /// `true` for the synthetic `..` entry.
    pub is_parent: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
    /// Computed on request; `None` for files and for folders not computed.
    pub folder_size: Option<FolderSize>,
}

impl FileEntry {
    /// Builds an entry from a `read_dir` item. Never fails: missing metadata
    /// simply results in zero size / unknown date.
    pub fn from_dir_entry(entry: &std::fs::DirEntry) -> Self {
        let path = entry.path();
        let name = entry.file_name();
        let is_symlink = entry.file_type().map(|t| t.is_symlink()).unwrap_or(false);
        // Follow symlinks so that links to directories are navigable.
        let metadata = std::fs::metadata(&path).or_else(|_| entry.metadata()).ok();

        let is_dir = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let size = metadata
            .as_ref()
            .filter(|m| !m.is_dir())
            .map(|m| m.len())
            .unwrap_or(0);
        let modified = metadata.and_then(|m| m.modified().ok());

        Self {
            name,
            path,
            is_dir,
            is_symlink,
            is_parent: false,
            size,
            modified,
            folder_size: None,
        }
    }

    /// The entry for `path` as it is now, or `None` when nothing is there any
    /// more. Metadata follows links like [`Self::from_dir_entry`] does.
    pub fn from_path(path: &Path) -> Option<Self> {
        let own = std::fs::symlink_metadata(path).ok()?;
        let metadata = std::fs::metadata(path).unwrap_or_else(|_| own.clone());
        let is_dir = metadata.is_dir();
        Some(Self {
            name: path.file_name()?.to_owned(),
            path: path.to_owned(),
            is_dir,
            is_symlink: own.file_type().is_symlink(),
            is_parent: false,
            size: if is_dir { 0 } else { metadata.len() },
            modified: metadata.modified().ok(),
            folder_size: None,
        })
    }

    /// The synthetic `..` entry pointing at `parent`.
    pub fn parent(parent: &Path) -> Self {
        Self {
            name: std::ffi::OsString::from(".."),
            path: parent.to_path_buf(),
            is_dir: true,
            is_symlink: false,
            is_parent: true,
            size: 0,
            modified: None,
            folder_size: None,
        }
    }
}
