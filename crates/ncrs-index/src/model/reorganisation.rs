use std::path::PathBuf;

use super::{ContentHash, RuleId};

/// What looks wrong about a file of an archive location. The order of the
/// variants is not a priority; the planner owns that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FindingKind {
    Duplicate,
    Misplaced,
    YearMismatch,
    NameOutlier,
}

impl FindingKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Duplicate => "duplicate",
            Self::Misplaced => "misplaced",
            Self::YearMismatch => "year_mismatch",
            Self::NameOutlier => "name_outlier",
        }
    }
}

/// A finding with the reason it was made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// Another file with the same content stays; `keeper` is its path below
    /// the location.
    Duplicate { keeper: PathBuf },
    /// A confirmed rule, or a suggestion that meets the evidence thresholds,
    /// files documents like this one elsewhere.
    Misplaced { rule: RuleId },
    /// The year folder of the path is not the year of the document's date.
    YearMismatch { folder_year: i32, date_year: i32 },
    /// The file name does not follow the pattern of its folder; `pattern` is
    /// the name pattern the folder follows.
    NameOutlier { pattern: String },
}

impl Finding {
    pub fn kind(&self) -> FindingKind {
        match self {
            Self::Duplicate { .. } => FindingKind::Duplicate,
            Self::Misplaced { .. } => FindingKind::Misplaced,
            Self::YearMismatch { .. } => FindingKind::YearMismatch,
            Self::NameOutlier { .. } => FindingKind::NameOutlier,
        }
    }
}

/// Why a proposed target is not an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictReason {
    /// Something is at the target on disk.
    TargetExists,
    /// Another item of the same plan wants the same target.
    TargetTakenByOtherItem,
    /// The target would not stay below the location.
    TargetLeavesLocation,
}

/// Why a finding has no proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoActionReason {
    /// The fields the folder's pattern needs for a name are not known.
    MissingFields,
    /// A surplus copy lying in an unordered folder: nothing is moved, the
    /// user decides about the copy.
    DuplicateCopy,
}

/// What the plan proposes for one file. There is no variant that deletes.
/// Targets are paths below the location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanOutcome {
    Move {
        target: PathBuf,
    },
    /// A move within the file's folder.
    Rename {
        target: PathBuf,
    },
    /// `target` is `None` when it would have left the location.
    Conflict {
        target: Option<PathBuf>,
        reason: ConflictReason,
    },
    NoAction(NoActionReason),
}

/// One file of the plan, as it was when the plan was made: `hash` and
/// `source` are what execution checks against the file on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanItem {
    /// Below the location.
    pub source: PathBuf,
    pub hash: ContentHash,
    /// Every finding that applies; the outcome follows the one that wins.
    pub findings: Vec<Finding>,
    pub outcome: PlanOutcome,
}
