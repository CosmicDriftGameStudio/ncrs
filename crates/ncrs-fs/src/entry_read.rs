//! Reading a `FileEntry` from the disk: the metadata a row shows, where the
//! platform keeps it.

use std::path::Path;

use ncrs_core::{EntryDetails, FileEntry, WindowsAttributes};

/// Builds an entry from a `read_dir` item. Never fails: missing metadata
/// simply results in zero size / unknown date.
pub fn entry_from_dir_entry(entry: &std::fs::DirEntry) -> FileEntry {
    let path = entry.path();
    let name = entry.file_name();
    let is_symlink = entry.file_type().map(|t| t.is_symlink()).unwrap_or(false);
    // Follow symlinks so that links to directories are navigable; a plain
    // entry is its own target, so it costs one stat.
    let own = entry.metadata().ok();
    let target = if is_symlink {
        std::fs::metadata(&path).ok()
    } else {
        own.clone()
    };
    let details = details_of(&path, is_symlink, own.as_ref(), target.as_ref());
    let metadata = target.or(own);

    let is_dir = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);
    let size = metadata
        .as_ref()
        .filter(|m| !m.is_dir())
        .map(|m| m.len())
        .unwrap_or(0);
    let modified = metadata.and_then(|m| m.modified().ok());

    FileEntry {
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
/// more. Metadata follows links like [`entry_from_dir_entry`] does.
pub fn entry_from_path(path: &Path) -> Option<FileEntry> {
    let own = std::fs::symlink_metadata(path).ok()?;
    let is_symlink = own.file_type().is_symlink();
    let target = std::fs::metadata(path).ok();
    let details = details_of(path, is_symlink, Some(&own), target.as_ref());
    let metadata = target.unwrap_or(own);
    let is_dir = metadata.is_dir();
    Some(FileEntry {
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

/// Owner and group names stay `None`: turning ids into names needs the system's user
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
        uid: shown.and_then(uid_of),
        gid: shown.and_then(gid_of),
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

#[cfg(unix)]
fn uid_of(metadata: &std::fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::MetadataExt as _;
    Some(metadata.uid())
}

#[cfg(unix)]
fn gid_of(metadata: &std::fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::MetadataExt as _;
    Some(metadata.gid())
}

#[cfg(not(unix))]
fn uid_of(_metadata: &std::fs::Metadata) -> Option<u32> {
    None
}

#[cfg(not(unix))]
fn gid_of(_metadata: &std::fs::Metadata) -> Option<u32> {
    None
}
