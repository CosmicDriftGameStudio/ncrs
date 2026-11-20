use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::details::EntryDetails;
use crate::folder_size::FolderSize;

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
    pub details: EntryDetails,
}

impl FileEntry {
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
            details: EntryDetails::default(),
        }
    }
}
