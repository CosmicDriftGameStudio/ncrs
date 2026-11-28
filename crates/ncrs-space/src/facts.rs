//! What a scan found out about one entry beyond its size: the answer of the in-use
//! check, when it was last used, and why an entry of your own is offered.

use std::path::PathBuf;
use std::time::SystemTime;

use ncrs_os_probe::{Holder, UnknownWhy};

/// The answer of the in-use check at scan time, kept so a row can say "no build running"
/// or "Chrome runs".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum UseCheck {
    /// The check did not run: the processes cannot be listed here, or the entry is not checked.
    #[default]
    NotChecked,
    /// No tool of the entry runs and no process has it open.
    Free,
    /// A tool of the entry runs or a process has a file in it open.
    Held(Vec<Holder>),
    /// The check ran and could not tell.
    Unknown(UnknownWhy),
}

/// How the version of an installed app compares with the one in an installer's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionRelation {
    Same,
    InstalledNewer,
    InstalledOlder,
    Unknown,
}

/// What is known about the app an installer in Downloads is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledApp {
    /// Where the app is installed.
    pub path: PathBuf,
    /// The app's own version, where the system tells it.
    pub version: Option<String>,
    /// The version in the installer's file name.
    pub installer_version: Option<String>,
    pub relation: VersionRelation,
}

/// Why an entry of your own is offered for a decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecideReason {
    /// An installer of an app that is installed.
    Installed(InstalledApp),
    /// An archive whose content lies next to it.
    Unpacked {
        folder: PathBuf,
    },
    /// The same content lies at `copy`; `in_downloads` is false when that is outside Downloads.
    Duplicate {
        copy: PathBuf,
        in_downloads: bool,
    },
    UntouchedYear,
    Other,
}

/// Facts of one entry; all of them may be unknown.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntryFacts {
    pub use_check: UseCheck,
    /// The best known last use: opened (where the system tells), else the last change.
    pub last_used: Option<SystemTime>,
    /// Where the operating system knows it (macOS Spotlight), else `None`.
    pub last_opened: Option<SystemTime>,
    pub open_count: Option<u32>,
    /// Set for entries of the Decide group that come from Downloads.
    pub reason: Option<DecideReason>,
}
