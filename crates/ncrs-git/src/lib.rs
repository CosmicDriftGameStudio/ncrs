//! Git state for the file panels and the Git view: which folders are
//! repositories, on which branch they are, how far they are from their upstream,
//! which files changed, and the history with its diffs. Everything reads through
//! `gix`; nothing starts a `git` process and nothing here writes to a repository.

mod detail;
mod diff;
mod error;
mod graph;
mod log;
mod refs;
mod repo_access;
mod repository;
mod state;
mod status;

pub use detail::{commit_detail, working_changes, ChangeKind, ChangedFile, CommitDetail};
pub use diff::{file_diff, DiffHunk, DiffRow, DiffTarget, FileDiff, RowKind};
pub use error::GitError;
pub use graph::{GraphRow, GraphSegment, SegmentHalf};
pub use log::{log, Log, LogCommit};
pub use refs::{refs, BranchInfo, RefKind, RefLabel, RefsOverview, WorktreeInfo};
pub use repository::{is_repository_root, repository_root_of};
pub use state::FileState;
pub use status::{state_of_entry, status, RepoStatus};
