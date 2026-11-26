//! Sorting files from the sources into the targets: what would go where
//! (`plan`), carrying it out (`run`), the watched sources (`run_watched`) and
//! the questions they leave (`open_questions`). Where a file belongs is only
//! ever decided by `ncrs_intake::decide`.

mod access;
mod plan_run;
mod questions;
mod read_unknown;
mod ready;
mod run;
mod suggest;
mod suggest_file;
mod suggest_material;
mod suggest_scheme;
mod suggest_store;
mod watched;

pub use access::SortingAccess;
pub use plan_run::{
    plan, plan_with_rules, PlanError, PlanFiles, PlanRequest, PlanRow, RowPlace, RowState,
    SortCounts, SortPlan, Taken,
};
pub use questions::{answer, leave, open_questions, question_count, Question, QuestionKind};
pub use read_unknown::{ReadProblem, ReadUnknown};
pub use ready::{is_on_own_volume, TargetState, WaitReason};
pub use run::{
    run, EntryReport, PlaceOutcome, PlaceReport, RunEntry, RunError, RunRequest, SortReport,
};
pub use suggest::{ModelNote, RuleForFile, SuggestError, Suggested, Suggestions};
pub use suggest_file::suggest_rule_for;
pub use suggest_scheme::suggest_scheme;
pub use suggest_store::{adopt, dismiss};
pub use watched::{
    forget_backlog, run_watched, SourceWait, WatchedFiles, WatchedReport, WatchedSkip,
};

#[cfg(test)]
mod tests;
