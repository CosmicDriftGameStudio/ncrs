//! Big folders and downloads of the user's own: sorted into groups with a reason,
//! never deleted here. They go to the trash from the tab, with undo.

mod apps;
mod downloads;
mod entries;

pub use apps::{app_name_of_installer, InstalledApps, SystemInstalledApps};
pub use downloads::{
    sort_downloads, sort_downloads_at, DownloadGroups, DownloadItem, DuplicateSet, Matched,
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
}
