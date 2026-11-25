//! Git state for the file panels and the Git view: which folders are
//! repositories, on which branch they are, how far they are from their upstream,
//! which files changed, and the history with its diffs. Reading goes through
//! `gix` and starts no process. Writing (stage, commit, branches, sync, merge,
//! stash) goes through the user's own `git` command, see `Git`.

mod action_error;
pub mod askpass;
mod branch;
mod blame;
mod changes;
mod cleanup;
mod cli;
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
mod summary;
mod state;
mod status;
mod sync;
mod undo;

pub use action_error::ActionError;
pub use changes::ChangeLists;
pub use cli::{write_actions_available, Git, GitCli, WRITE_ACTIONS_ENABLED};
pub use cleanup::{
    cleanup_list, remote_tips, CleanupBranch, CleanupList, CleanupPlan, CleanupReason, CleanupReport,
    CleanupRow, CleanupTarget, CleanupWorktree, KeepReason, Kept,
};
pub use commit::{CommitAndPush, CommitMessage};
pub use conflict::{parse_blocks, preview_lines, ConflictBlock, ConflictChoice, PreviewLine};
pub use detail::{commit_detail, working_changes, ChangeKind, ChangedFile, CommitDetail};
pub use diff::{file_diff, DiffHunk, DiffRow, DiffTarget, FileDiff, RowKind};
pub use error::GitError;
pub use graph::{GraphRow, GraphSegment, SegmentHalf};
pub use hunk::HunkSide;
pub use history_ops::{PickOutcome, ResetLoss, ResetMode};
pub use integrate::{
    ConflictKind, ConflictedFile, IntegrateOutcome, Integration, IntegrationDetail,
};
pub use log::{log, Log, LogCommit};
pub use refs::{refs, BranchInfo, RefKind, RefLabel, RefsOverview, WorktreeInfo};
pub use repository::{is_repository_root, repository_root_of};
pub use stash::StashEntry;
pub use state::FileState;
pub use status::{state_of_entry, status, RepoStatus};
pub use sync::{ForcePushPlan, PullStrategy};
pub use undo::{RecreatedBranch, UndoRecord, UndoSummary};
pub use containing::branches_containing;
pub use ignore::literal_pattern;
pub use summary::{last_fetch, repo_summary, RepoSummary, WorktreeSummary};
pub use blame::{blame, Blame, BlameGroup, BlameLimit, BlameLine};
pub use compare::{compare_diff, compare_files};
pub use file_history::{file_history, FileHistory, FileHistoryEntry};
