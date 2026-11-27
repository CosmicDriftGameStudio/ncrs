use std::io;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime};

use ncrs_os_probe::{FolderUse, Holder};

use crate::catalog::CatalogId;
use crate::scan::{Entry, EntryKind};

/// How long one look at the open files of a folder may take.
pub(crate) const FOLDER_USE_LIMIT: Duration = Duration::from_secs(5);

/// Where the answers about running processes come from; a fake in tests.
pub trait ProcessSource: Send + Sync {
    /// `None` when the processes cannot be listed.
    fn processes(&self) -> Option<Vec<Holder>>;
    fn folder_use(&self, folder: &Path, limit: Duration) -> FolderUse;
}

/// The real system, through `ncrs-os-probe`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemProbes;

impl ProcessSource for SystemProbes {
    fn processes(&self) -> Option<Vec<Holder>> {
        ncrs_os_probe::running_processes()
    }

    fn folder_use(&self, folder: &Path, limit: Duration) -> FolderUse {
        ncrs_os_probe::folder_in_use(folder, limit)
    }
}

/// What an unknown answer about running tools or open folders means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownUse {
    /// Unknown counts as in use: the entry is skipped.
    Skip,
    /// The user confirmed that ncrs cannot check here.
    AllowAfterConfirm,
}

/// Why an entry was left alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skip {
    ToolRunning(Holder),
    FolderOpen(Holder),
    UseUnknown,
    Symlink,
    OtherDrive,
    Changed,
    Gone,
    NotGranted,
    Failed(String),
}

impl Skip {
    pub(crate) fn from_io(error: &io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::NotFound => Self::Gone,
            io::ErrorKind::PermissionDenied => Self::NotGranted,
            _ => Self::Failed(error.to_string()),
        }
    }
}

/// What a check compares the folder on the disk against.
pub(crate) struct Subject<'a> {
    pub path: &'a Path,
    pub tools: &'a [&'static str],
    /// `None` when the size cannot be measured again from the path.
    pub bytes: Option<u64>,
    pub modified: Option<SystemTime>,
    pub device: Option<u64>,
}

pub fn check_before_delete(
    entry: &Entry,
    probes: &dyn ProcessSource,
    unknown: UnknownUse,
) -> Result<(), Skip> {
    let tools = match &entry.kind {
        EntryKind::Catalog(id) => id.tools(),
        EntryKind::Worktree(_) | EntryKind::Decide(_) => &[],
    };
    let measurable = !matches!(&entry.kind, EntryKind::Catalog(CatalogId::UnavailableSimulators));
    check_subject(
        &Subject {
            path: &entry.path,
            tools,
            bytes: measurable.then_some(entry.size.bytes),
            modified: entry.modified,
            device: entry.device,
        },
        probes,
        unknown,
    )
}

pub(crate) fn check_subject(
    subject: &Subject<'_>,
    probes: &dyn ProcessSource,
    unknown: UnknownUse,
) -> Result<(), Skip> {
    let metadata = std::fs::symlink_metadata(subject.path).map_err(|error| Skip::from_io(&error))?;
    if metadata.file_type().is_symlink() {
        return Err(Skip::Symlink);
    }
    if subject.device.is_some() && device_of(&metadata) != subject.device {
        return Err(Skip::OtherDrive);
    }
    match probes.processes() {
        Some(running) => {
            if let Some(holder) = running.into_iter().find(|holder| runs_tool(holder, subject.tools)) {
                return Err(Skip::ToolRunning(holder));
            }
        }
        None if unknown == UnknownUse::Skip => return Err(Skip::UseUnknown),
        None => {}
    }
    match probes.folder_use(subject.path, FOLDER_USE_LIMIT) {
        FolderUse::Free => {}
        FolderUse::Held(holders) => {
            return Err(holders.into_iter().next().map_or(Skip::UseUnknown, Skip::FolderOpen));
        }
        FolderUse::Unknown(_) if unknown == UnknownUse::Skip => return Err(Skip::UseUnknown),
        FolderUse::Unknown(_) => {}
    }
    if subject.modified.is_some() && metadata.modified().ok() != subject.modified {
        return Err(Skip::Changed);
    }
    if let Some(bytes) = subject.bytes {
        let now = measured_bytes(subject.path, &metadata).ok_or(Skip::Changed)?;
        if now != bytes {
            return Err(Skip::Changed);
        }
    }
    Ok(())
}

pub(crate) fn measured_bytes(path: &Path, metadata: &std::fs::Metadata) -> Option<u64> {
    if metadata.is_dir() {
        let never = AtomicBool::new(false);
        ncrs_fs::measure_folder(path, &never).map(|size| size.bytes)
    } else {
        Some(metadata.len())
    }
}

pub(crate) fn runs_tool(holder: &Holder, tools: &[&str]) -> bool {
    let name = holder.name.to_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    tools.iter().any(|tool| tool.to_lowercase() == name)
}

#[cfg(unix)]
pub(crate) fn device_of(metadata: &std::fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt as _;
    Some(metadata.dev())
}

#[cfg(not(unix))]
pub(crate) fn device_of(_metadata: &std::fs::Metadata) -> Option<u64> {
    None
}
