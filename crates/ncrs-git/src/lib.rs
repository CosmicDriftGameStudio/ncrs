//! Git state for the file panels and the Git view: which folders are
//! repositories, on which branch they are, how far they are from their upstream,
//! which files changed, and the history with its diffs. Reading goes through
//! `gix` and starts no process. Writing (stage, commit, branches, sync, merge,
//! stash) goes through the user's own `git` command, see `Git`.

mod action_error;
pub mod askpass;
mod blame;
mod branch;
mod changes;
mod cleanup;
mod cli;
mod clone;
mod commit;
mod compare;
mod conflict;
mod containing;
mod detail;
mod diff;
mod error;
mod file_history;
mod graph;
mod history_ops;
mod hunk;
mod ignore;
mod integrate;
mod log;
mod refname;
mod refs;
mod repo_access;
mod repository;
mod stash;
mod state;
mod status;
mod summary;
mod sync;
mod undo;
mod working_file;

pub use action_error::ActionError;
pub use blame::{blame, Blame, BlameGroup, BlameLimit, BlameLine};
pub use changes::ChangeLists;
pub use cleanup::{
    cleanup_list, remote_tips, CleanupBranch, CleanupList, CleanupPlan, CleanupReason,
    CleanupReport, CleanupRow, CleanupTarget, CleanupWorktree, KeepReason, Kept,
};
pub use cli::{write_actions_available, Git, GitCli, WRITE_ACTIONS_ENABLED};
pub use clone::clone;
pub use commit::{CommitAndPush, CommitMessage};
pub use compare::{compare_diff, compare_files};
pub use conflict::{parse_blocks, preview_lines, ConflictBlock, ConflictChoice, PreviewLine};
pub use containing::branches_containing;
pub use detail::{commit_detail, working_changes, ChangeKind, ChangedFile, CommitDetail};
pub use diff::{file_diff, DiffHunk, DiffRow, DiffTarget, FileDiff, RowKind};
pub use error::GitError;
pub use file_history::{file_history, FileHistory, FileHistoryEntry};
pub use graph::{GraphRow, GraphSegment, SegmentHalf};
pub use history_ops::{PickOutcome, ResetLoss, ResetMode};
pub use hunk::HunkSide;
pub use ignore::literal_pattern;
pub use integrate::{
    ConflictKind, ConflictedFile, IntegrateOutcome, Integration, IntegrationDetail,
};
pub use log::{log, Log, LogCommit};
pub use refs::{refs, BranchInfo, RefKind, RefLabel, RefsOverview, WorktreeInfo};
pub use repository::{is_repository_root, repository_root_of};
pub use stash::StashEntry;
pub use state::FileState;
pub use status::{state_of_entry, status, RepoStatus};
pub use summary::{last_fetch, repo_summary, RepoSummary, WorktreeSummary};
pub use sync::{ForcePushPlan, PullStrategy};
pub use undo::{RecreatedBranch, UndoRecord, UndoSummary};
