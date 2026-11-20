//! Filesystem layer. Knows nothing about the UI.

pub mod archive;
mod delete;
mod entry_read;
pub mod flat_names;
mod folder_size;
pub mod folder_watch;
#[cfg(any(all(feature = "sandbox", target_os = "macos"), test))]
mod grants;
#[cfg(windows)]
#[cfg(test)]
mod long_path_tests;
mod open;
mod open_programs;
mod open_rules;
mod ops;
mod origin_mark;
mod owners;
pub mod permissions;
mod place_file;
mod reader;
#[cfg(any(test, feature = "test-support"))]
pub mod real_trash_tests;
pub mod remote;
mod rename;
mod replace_file;
mod sandbox;
pub mod search;
mod sort;
mod terminal;
pub mod transfer;
#[cfg(any(target_os = "macos", test, feature = "test-support"))]
mod trash_journal;
mod typed_path;
mod user_machine;
mod volumes;

pub use delete::{
    is_same_place, remove_permanently, system_trash, RestoreError, Trash, TrashReach, TrashedItem,
};
pub use entry_read::{entry_from_dir_entry, entry_from_path};
pub use folder_size::{
    measure_folder, measure_folder_tree, measure_folder_tree_reporting, MeasuredTree,
    RUNNING_REPORT_INTERVAL,
};
pub use ncrs_core::{EntryDetails, FileEntry, FolderSize, WindowsAttributes};
pub use open::OpenCommand;
pub use open::{
    could_execute, could_execute_listed, is_app_bundle, open_command, program_command,
    runs_in_app_sandbox, system_open_command, Launcher, OpenKind, SystemLauncher,
};
#[cfg(target_os = "macos")]
pub use open::{privacy_settings_command, trash_in_finder_command};
pub use open_programs::{is_separator, OpenPrograms, ProgramLine, ProgramLineProblem};
pub use open_rules::{default_rules, matching_with, parse_extensions, OpenRule, OpenWith};
pub use ops::{create_dir, create_file, CreateDirError};
pub use reader::{home_dir, read_directory, start_dir, ReadError};
pub use rename::rename;
pub use replace_file::replace_file_atomically;
pub use sandbox::{
    grant_start_dir, grant_start_dir_for, preferred_home, restore_grants, sandbox_active,
    sandbox_home, SandboxAccess,
};
pub use sort::{sort_entries, SortColumn, SortKey};
pub use terminal::{
    detect_terminals, detected_terminals, terminal_command, warm_up_terminal_detection,
    KnownTerminal, NoTerminal,
};
#[cfg(any(test, feature = "test-support"))]
pub use trash_journal::{FolderBackend, JournaledTrash};
pub use typed_path::{locate, resolve as resolve_typed_path, Destination, PathProblem};
pub use user_machine::{forbid_user_machine_in_tests, user_machine_is_off_limits};
pub use volumes::{list_volumes, Volume};
