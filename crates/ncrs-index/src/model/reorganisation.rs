use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use super::{corrupt, ContentHash, PlanId, RuleId};
use crate::error::IndexError;

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

    fn parse(value: &str) -> Result<Self, IndexError> {
        match value {
            "duplicate" => Ok(Self::Duplicate),
            "misplaced" => Ok(Self::Misplaced),
            "year_mismatch" => Ok(Self::YearMismatch),
            "name_outlier" => Ok(Self::NameOutlier),
            other => Err(corrupt("reorganisation_finding.kind", other)),
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

    pub(crate) fn detail_column(&self) -> Result<String, IndexError> {
        Ok(match self {
            Self::Duplicate { keeper } => keeper
                .to_str()
                .ok_or_else(|| IndexError::NonUtf8Path(keeper.clone()))?
                .to_owned(),
            Self::Misplaced { rule } => rule.get().to_string(),
            Self::YearMismatch {
                folder_year,
                date_year,
            } => format!("{folder_year}:{date_year}"),
            Self::NameOutlier { pattern } => pattern.clone(),
        })
    }

    pub(crate) fn from_columns(kind: &str, detail: &str) -> Result<Self, IndexError> {
        let bad = || corrupt("reorganisation_finding.detail", detail);
        Ok(match FindingKind::parse(kind)? {
            FindingKind::Duplicate => Self::Duplicate {
                keeper: PathBuf::from(detail),
            },
            FindingKind::Misplaced => Self::Misplaced {
                rule: RuleId::new(detail.parse().map_err(|_unparsable| bad())?),
            },
            FindingKind::YearMismatch => {
                let (folder, date) = detail.split_once(':').ok_or_else(bad)?;
                Self::YearMismatch {
                    folder_year: folder.parse().map_err(|_unparsable| bad())?,
                    date_year: date.parse().map_err(|_unparsable| bad())?,
                }
            }
            FindingKind::NameOutlier => Self::NameOutlier {
                pattern: detail.to_owned(),
            },
        })
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

impl ConflictReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::TargetExists => "target_exists",
            Self::TargetTakenByOtherItem => "target_taken_by_other_item",
            Self::TargetLeavesLocation => "target_leaves_location",
        }
    }

    fn parse(value: &str) -> Result<Self, IndexError> {
        match value {
            "target_exists" => Ok(Self::TargetExists),
            "target_taken_by_other_item" => Ok(Self::TargetTakenByOtherItem),
            "target_leaves_location" => Ok(Self::TargetLeavesLocation),
            other => Err(corrupt("reorganisation_item.detail", other)),
        }
    }
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

impl NoActionReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::MissingFields => "missing_fields",
            Self::DuplicateCopy => "duplicate_copy",
        }
    }

    fn parse(value: &str) -> Result<Self, IndexError> {
        match value {
            "missing_fields" => Ok(Self::MissingFields),
            "duplicate_copy" => Ok(Self::DuplicateCopy),
            other => Err(corrupt("reorganisation_item.detail", other)),
        }
    }
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

impl PlanOutcome {
    pub(crate) fn to_columns(
        &self,
    ) -> Result<(&'static str, Option<&str>, Option<&'static str>), IndexError> {
        fn text(path: &Path) -> Result<&str, IndexError> {
            path.to_str()
                .ok_or_else(|| IndexError::NonUtf8Path(path.to_path_buf()))
        }
        Ok(match self {
            Self::Move { target } => ("move", Some(text(target)?), None),
            Self::Rename { target } => ("rename", Some(text(target)?), None),
            Self::Conflict { target, reason } => (
                "conflict",
                target.as_deref().map(text).transpose()?,
                Some(reason.as_str()),
            ),
            Self::NoAction(reason) => ("no_action", None, Some(reason.as_str())),
        })
    }

    pub(crate) fn from_columns(
        outcome: &str,
        target: Option<String>,
        detail: Option<&str>,
    ) -> Result<Self, IndexError> {
        let missing = |column: &'static str| corrupt(column, outcome);
        Ok(match outcome {
            "move" => Self::Move {
                target: PathBuf::from(target.ok_or_else(|| missing("reorganisation_item.target"))?),
            },
            "rename" => Self::Rename {
                target: PathBuf::from(target.ok_or_else(|| missing("reorganisation_item.target"))?),
            },
            "conflict" => Self::Conflict {
                target: target.map(PathBuf::from),
                reason: ConflictReason::parse(
                    detail.ok_or_else(|| missing("reorganisation_item.detail"))?,
                )?,
            },
            "no_action" => Self::NoAction(NoActionReason::parse(
                detail.ok_or_else(|| missing("reorganisation_item.detail"))?,
            )?),
            other => return Err(corrupt("reorganisation_item.outcome", other)),
        })
    }
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReorganisationPlan {
    pub id: PlanId,
    pub location: PathBuf,
    pub created_at: DateTime<Utc>,
    /// Sorted by source.
    pub items: Vec<PlanItem>,
}
