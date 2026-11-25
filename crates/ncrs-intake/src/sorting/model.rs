use std::path::PathBuf;

use ncrs_index::TargetId;

/// A folder filed documents end up in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortTarget {
    pub id: TargetId,
    pub name: String,
    pub path: PathBuf,
    /// Whether the folder was on a volume of its own, not the system disk,
    /// when it was saved; `None` when that was never checked.
    pub own_volume: Option<bool>,
}

/// What happens with the file in the source once it is filed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LocationTransfer {
    Copy,
    #[default]
    Move,
}

/// What to do with a file that has no text to decide by.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OnNoText {
    #[default]
    Ask,
    Skip,
}

/// What to do with a file that fits more than one target.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OnSeveralTargets {
    #[default]
    Ask,
    CopyToAll,
}

/// A folder documents are taken from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortSource {
    pub path: PathBuf,
    /// Globs as in the search dialog, `;` separated.
    pub include: String,
    pub exclude: String,
    /// Empty: the source is paused, never "all targets".
    pub targets: Vec<TargetId>,
    pub watch: bool,
    pub transfer: LocationTransfer,
    pub no_text: OnNoText,
    pub several: OnSeveralTargets,
    /// As `SortTarget::own_volume`.
    pub own_volume: Option<bool>,
}
