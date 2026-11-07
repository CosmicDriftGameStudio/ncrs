use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::details::{EntryDetails, WindowsAttributes};
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
    /// Builds an entry from a `read_dir` item. Never fails: missing metadata
    /// simply results in zero size / unknown date.
    pub fn from_dir_entry(entry: &std::fs::DirEntry) -> Self {
        let path = entry.path();
        let name = entry.file_name();
        let is_symlink = entry.file_type().map(|t| t.is_symlink()).unwrap_or(false);
        // Follow symlinks so that links to directories are navigable.
        let target = std::fs::metadata(&path).ok();
        let own = entry.metadata().ok();
        let details = details_of(&path, is_symlink, own.as_ref(), target.as_ref());
        let metadata = target.or(own);

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
            details,
        }
    }

    /// The entry for `path` as it is now, or `None` when nothing is there any
    /// more. Metadata follows links like [`Self::from_dir_entry`] does.
    pub fn from_path(path: &Path) -> Option<Self> {
        let own = std::fs::symlink_metadata(path).ok()?;
        let is_symlink = own.file_type().is_symlink();
        let target = std::fs::metadata(path).ok();
        let details = details_of(path, is_symlink, Some(&own), target.as_ref());
        let metadata = target.unwrap_or(own);
        let is_dir = metadata.is_dir();
        Some(Self {
            name: path.file_name()?.to_owned(),
            path: path.to_owned(),
            is_dir,
            is_symlink,
            is_parent: false,
            size: if is_dir { 0 } else { metadata.len() },
            modified: metadata.modified().ok(),
            folder_size: None,
            details,
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
            details: EntryDetails::default(),
        }
    }
}

/// Owner and group stay `None`: turning ids into names needs the system's user
/// database, which the filesystem layer owns.
fn details_of(
    path: &Path,
    is_symlink: bool,
    own: Option<&std::fs::Metadata>,
    target: Option<&std::fs::Metadata>,
) -> EntryDetails {
    let shown = target.or(own);
    EntryDetails {
        mode: shown.and_then(mode_of),
        owner: None,
        group: None,
        created: shown.and_then(|m| m.created().ok()),
        accessed: shown.and_then(|m| m.accessed().ok()),
        link_target: if is_symlink {
            std::fs::read_link(path).ok()
        } else {
            None
        },
        link_broken: is_symlink && target.is_none(),
        windows: shown.and_then(windows_attributes_of),
    }
}

#[cfg(unix)]
fn mode_of(metadata: &std::fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt as _;
    Some(metadata.permissions().mode() & 0o7777)
}

#[cfg(not(unix))]
fn mode_of(_metadata: &std::fs::Metadata) -> Option<u32> {
    None
}

#[cfg(windows)]
fn windows_attributes_of(metadata: &std::fs::Metadata) -> Option<WindowsAttributes> {
    use std::os::windows::fs::MetadataExt as _;
    const READ_ONLY: u32 = 0x1;
    const HIDDEN: u32 = 0x2;
    const SYSTEM: u32 = 0x4;
    let bits = metadata.file_attributes();
    Some(WindowsAttributes {
        read_only: bits & READ_ONLY != 0,
        hidden: bits & HIDDEN != 0,
        system: bits & SYSTEM != 0,
    })
}

#[cfg(not(windows))]
fn windows_attributes_of(_metadata: &std::fs::Metadata) -> Option<WindowsAttributes> {
    None
}
