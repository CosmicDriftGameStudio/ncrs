//! Filesystem layer. Knows nothing about the UI.

pub mod archive;
mod delete;
pub mod flat_names;
mod folder_size;
#[cfg(any(all(feature = "sandbox", target_os = "macos"), test))]
mod grants;
mod open;
mod open_programs;
mod ops;
mod reader;
#[cfg(any(test, feature = "test-support"))]
pub mod real_trash_tests;
pub mod remote;
mod replace_file;
mod sandbox;
mod sort;
pub mod transfer;
#[cfg(any(target_os = "macos", test, feature = "test-support"))]
mod trash_journal;
mod typed_path;
mod user_machine;
mod volumes;

pub use delete::{
    is_same_place, remove_permanently, system_trash, RestoreError, Trash, TrashReach, TrashedItem,
};
pub use folder_size::measure_folder;
pub use ncrs_core::{FileEntry, FolderSize};
#[cfg(any(test, feature = "test-support"))]
pub use open::OpenCommand;
pub use open::{
    could_execute, open_command, runs_in_app_sandbox, system_open_command, Launcher, OpenKind,
    SystemLauncher,
};
#[cfg(target_os = "macos")]
pub use open::{privacy_settings_command, trash_in_finder_command};
pub use open_programs::{is_separator, OpenPrograms, ProgramLine, ProgramLineProblem};
pub use ops::{create_dir, create_file, CreateDirError};
pub use reader::{home_dir, read_directory, start_dir, ReadError};
pub use replace_file::replace_file_atomically;
pub use sandbox::{
    grant_start_dir, grant_start_dir_for, preferred_home, restore_grants, sandbox_active,
    sandbox_home, SandboxAccess,
};
pub use sort::{sort_entries, SortColumn, SortKey};
#[cfg(any(test, feature = "test-support"))]
pub use trash_journal::{FolderBackend, JournaledTrash};
pub use typed_path::{locate, resolve as resolve_typed_path, Destination, PathProblem};
pub use user_machine::forbid_user_machine_in_tests;
pub use volumes::{list_volumes, Volume};
