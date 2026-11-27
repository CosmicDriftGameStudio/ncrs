//! Big folders and downloads of the user's own: sorted into groups with a reason,
//! never deleted here. They go to the trash from the tab, with undo.

mod apps;
mod downloads;
mod entries;
mod named;

pub use apps::{
    app_name_of_installer, compare_versions, version_of_installer, InstalledApps,
    SystemInstalledApps,
};
pub use downloads::{
    sort_downloads, sort_downloads_at, sort_downloads_with, DownloadGroups, DownloadItem,
    DuplicateSet, Matched,
};
pub use entries::decide_entries;

/// Why an entry of your own is shown for a decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecideKind {
    BigFolder,
    InstalledApp,
    Unpacked,
    Duplicate,
    UntouchedYear,
    Other,
    /// The folders of the Claude, Codex and similar session logs.
    SessionLogs,
    /// The simulators that can still be used (the unavailable ones are safe).
    Simulators,
    /// Backups of your own, such as `~/Backups` or the iPhone backups.
    Backups,
    /// Downloaded models of AI tools.
    ModelCache,
}

impl DecideKind {
    /// Whether the folder may go to the trash as a whole. The simulator entry stands for
    /// the devices that are in use, which only Xcode may remove.
    pub fn can_trash(&self) -> bool {
        !matches!(self, Self::Simulators)
    }
}
