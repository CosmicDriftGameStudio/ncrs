//! Git state for the file panels and the Git view: which folders are
//! repositories, on which branch they are, how far they are from their upstream,
//! which files changed, and the history with its diffs. Reading goes through
//! `gix` and starts no process. Writing (stage, commit, branches, sync, merge,
//! stash) goes through the user's own `git` command, see `Git`.

mod action_error;
mod branch;
mod changes;
mod cli;
mod commit;
mod conflict;
mod detail;
mod diff;
mod error;
mod graph;
mod hunk;
mod integrate;
mod log;
mod refs;
mod repo_access;
mod repository;
mod stash;
mod state;
mod status;
mod sync;
mod undo;

pub use action_error::ActionError;
pub use cli::{write_actions_available, Git, GitCli, WRITE_ACTIONS_ENABLED};
pub use commit::CommitMessage;
pub use conflict::{parse_blocks, ConflictBlock, ConflictChoice};
pub use detail::{commit_detail, working_changes, ChangeKind, ChangedFile, CommitDetail};
pub use diff::{file_diff, DiffHunk, DiffRow, DiffTarget, FileDiff, RowKind};
pub use error::GitError;
pub use graph::{GraphRow, GraphSegment, SegmentHalf};
pub use hunk::HunkSide;
pub use integrate::{ConflictKind, ConflictedFile, IntegrateOutcome, Integration};
pub use log::{log, Log, LogCommit};
pub use refs::{refs, BranchInfo, RefKind, RefLabel, RefsOverview, WorktreeInfo};
pub use repository::{is_repository_root, repository_root_of};
pub use stash::StashEntry;
pub use state::FileState;
pub use status::{state_of_entry, status, RepoStatus};
pub use undo::{UndoRecord, UndoSummary};
