//! Questions to the operating system that the "Platz schaffen" checks need:
//! which processes run, whether one holds a folder open, and how much space a
//! volume has. Unknown answers stay unknown; the caller decides what they mean.

#[cfg(any(target_os = "linux", windows))]
use std::io;
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

#[cfg(all(target_os = "macos", not(feature = "sandbox")))]
#[path = "macos.rs"]
mod platform;
#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod platform;
#[cfg(windows)]
#[path = "windows.rs"]
mod platform;
#[cfg(not(any(
    all(target_os = "macos", not(feature = "sandbox")),
    target_os = "linux",
    windows
)))]
#[path = "other.rs"]
mod platform;

mod processes;
mod usage;
mod volume;

#[cfg(windows)]
pub use platform::volume_serial_of;
pub use processes::running_processes;
pub use usage::{file_usage, FileUsage};
pub use volume::volume_space;

/// A process, by id and executable name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    pub pid: u32,
    pub name: String,
}

/// Whether any process has a file open or its working directory under a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderUse {
    Free,
    Held(Vec<Holder>),
    Unknown(UnknownWhy),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnknownWhy {
    /// The platform or the sandbox does not let ncrs look.
    NotAllowed,
    TimedOut,
    /// More files under the folder than one check looks at; no platform reports it now.
    TooManyFiles,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolumeSpace {
    pub total: u64,
    pub available: u64,
}

/// Blocking; gives up after `limit`. The check then runs on in the background
/// until it ends by itself.
pub fn folder_in_use(folder: &Path, limit: Duration) -> FolderUse {
    let owned_folder = folder.to_path_buf();
    let (sender, receiver) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("folder-in-use".to_owned())
        .spawn(move || {
            // The receiver is gone after a timeout; nobody waits for the answer then.
            drop(sender.send(platform::folder_use(&owned_folder)));
        });
    if let Err(err) = spawned {
        return FolderUse::Unknown(UnknownWhy::Failed(err.to_string()));
    }
    match receiver.recv_timeout(limit) {
        Ok(answer) => answer,
        Err(RecvTimeoutError::Timeout) => FolderUse::Unknown(UnknownWhy::TimedOut),
        Err(RecvTimeoutError::Disconnected) => FolderUse::Unknown(UnknownWhy::Failed(
            "the check stopped without an answer".to_owned(),
        )),
    }
}

#[cfg(any(target_os = "linux", windows))]
fn failed(err: &io::Error) -> FolderUse {
    FolderUse::Unknown(UnknownWhy::Failed(err.to_string()))
}
