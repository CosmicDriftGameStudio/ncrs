//! Sorting: sources, targets and the one decision where a file goes.

mod decide;
mod extension;
mod model;

pub use decide::{decide, Decision, Place, Subject, TargetRules, TextState, Words};
pub use extension::file_name_with_extension;
pub use model::{OnNoText, OnSeveralTargets, SortSource, SortTarget, LocationTransfer};
