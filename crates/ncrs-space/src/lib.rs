//! What takes the space on a drive and how much of it can go: regenerable caches
//! of known tools (safe), merged worktrees, big folders of the user's own (decide),
//! and anything a running process holds (in use). The model never decides what is
//! safe; every fact here is checked by code, again right before a delete.

mod catalog;
mod catalog_texts;
mod checks;
pub mod decide;
mod delete;
mod drives;
mod facts;
mod log;
mod scan;
mod simulators;
mod worktrees;

pub use catalog::{
    cargo_project_name, catalog, catalog_for, touches_protected, CatalogId, Os, Place,
};
pub use catalog_texts::catalog_msgs;
pub use checks::{check_before_delete, ProcessSource, Skip, SystemProbes, UnknownUse};
pub use delete::delete_safe;
pub use drives::{drive_name_of, drives, Drive};
pub use facts::{DecideReason, EntryFacts, InstalledApp, UseCheck, VersionRelation};
pub use log::{append_log, parse_log, LogRow, LoggedRow, LoggedRun, RunLog};
pub use scan::{scan, Entry, EntryKind, Group, Scan, ScanRequest};
pub use worktrees::{
    merged_worktrees, remove_worktrees, scan_worktrees, worktree_roots, KeptWhy, KeptWorktree,
    WorktreeCount, WorktreeRoot, WorktreeScan, WorktreeSpace,
};

pub use ncrs_config::space::SpaceSettings;
pub use ncrs_os_probe::{FolderUse, Holder, UnknownWhy, VolumeSpace};
