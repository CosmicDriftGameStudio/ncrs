use std::path::PathBuf;
use std::time::SystemTime;

/// The attributes of a file Windows keeps beside the permission bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WindowsAttributes {
    pub read_only: bool,
    pub hidden: bool,
    pub system: bool,
}

/// What a panel can show beyond name, size and date. Every field is `None` or
/// empty when the source (an archive, an SFTP server) does not know it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EntryDetails {
    /// Unix permission bits (`& 0o7777`) of the link target, of the link itself
    /// when the target is missing.
    pub mode: Option<u32>,
    /// The numeric ids of the owner and the group, where the system has them.
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    /// Filled by the filesystem layer, which can look names up; the core only
    /// holds the result. A name that cannot be found is the number as text.
    pub owner: Option<String>,
    pub group: Option<String>,
    pub created: Option<SystemTime>,
    pub accessed: Option<SystemTime>,
    /// Where a symbolic link points, as written in the link.
    pub link_target: Option<PathBuf>,
    /// A link whose target does not exist.
    pub link_broken: bool,
    /// Only filled on Windows.
    pub windows: Option<WindowsAttributes>,
}
