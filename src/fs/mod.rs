//! Filesystem layer. Knows nothing about the UI.

pub mod archive;
mod delete;
mod entry;
#[cfg(any(all(feature = "sandbox", target_os = "macos"), test))]
mod grants;
mod open;
mod ops;
mod reader;
pub mod remote;
mod sandbox;
pub mod search;
mod sort;
pub mod transfer;
#[cfg(any(target_os = "macos", test))]
mod trash_journal;
mod typed_path;
mod volumes;

pub use delete::{remove_permanently, system_trash, RestoreError, Trash, TrashReach, TrashedItem};
pub use entry::FileEntry;
#[cfg(test)]
pub use open::OpenCommand;
pub use open::{
    could_execute, open_command, runs_in_app_sandbox, system_open_command, Launcher, OpenKind,
    SystemLauncher,
};
#[cfg(target_os = "macos")]
pub use open::{privacy_settings_command, trash_in_finder_command};
pub use ops::{create_dir, create_file, CreateDirError};
pub use reader::{home_dir, read_directory, start_dir, ReadError};
pub use sandbox::{
    grant_start_dir, grant_start_dir_for, preferred_home, restore_grants, sandbox_active,
    sandbox_home, SandboxAccess,
};
pub use sort::{sort_entries, SortColumn, SortKey};
#[cfg(test)]
pub use trash_journal::{FolderBackend, JournaledTrash};
pub use typed_path::{locate, resolve as resolve_typed_path, Destination, PathProblem};
pub use volumes::{list_volumes, Volume};
