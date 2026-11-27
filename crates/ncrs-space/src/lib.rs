//! What takes the space on a drive and how much of it can go: regenerable caches
//! of known tools (safe), merged worktrees, big folders of the user's own (decide),
//! and anything a running process holds (in use). The model never decides what is
//! safe; every fact here is checked by code, again right before a delete.

mod catalog;
mod checks;
pub mod decide;
mod delete;
mod log;
mod scan;
mod worktrees;

pub use catalog::{catalog, CatalogId, Os, Place};
pub use checks::{check_before_delete, ProcessSource, Skip, SystemProbes, UnknownUse};
pub use delete::delete_safe;
pub use log::{append_log, LogRow, RunLog};
pub use scan::{scan, Entry, EntryKind, Group, Scan, ScanRequest};
pub use worktrees::{merged_worktrees, remove_worktrees, WorktreeSpace};

pub use ncrs_config::space::SpaceSettings;
pub use ncrs_os_probe::{FolderUse, Holder, UnknownWhy, VolumeSpace};
