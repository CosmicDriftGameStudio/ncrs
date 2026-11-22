//! What the Git tab shows of the actions of #338: local changes, the toolbar,
//! menus, the new-branch dialog, a merge with conflicts and notices. Design
//! phase: the state is filled by the caller, nothing here reads a repository.

use iced::widget::text_editor;
use ncrs_git::{ChangedFile, FileDiff, UndoRecord};

use crate::job::Job;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeSide {
    Unstaged,
    Staged,
}

#[derive(Debug, Default)]
pub struct LocalChanges {
    pub unstaged: Vec<ChangedFile>,
    pub staged: Vec<ChangedFile>,
    pub selected: Option<(ChangeSide, usize)>,
    /// The diff of the selected file.
    pub diff: Option<FileDiff>,
    pub title: String,
    pub body: text_editor::Content,
    pub amend: bool,
}

impl Actions {
    /// A dialog is over the tab: it takes the keys.
    pub fn dialog_open(&self) -> bool {
        self.new_branch.is_some() || self.dialog.is_some()
    }
}

/// What the toolbar of the header shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SyncBar {
    pub ahead: u32,
    pub behind: u32,
    /// What "undo" would take back, ready to show; `None`: nothing to undo.
    pub undo: Option<String>,
}

/// What a menu is opened on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuTarget {
    LocalBranch(usize),
    /// The index into the remote branches of the overview.
    RemoteBranch(usize),
    Stash(usize),
    /// The index into the conflicts of the merge.
    Conflict(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    Checkout,
    NewBranchFrom,
    Rename,
    Delete,
    MergeInto,
    RebaseOnto,
    Push,
    CheckoutAsLocal,
    DeleteOnRemote,
    StashApply,
    StashApplyAndDrop,
    StashDrop,
}

/// The menu that is open: it hangs at the row of its target in the left list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenMenu {
    pub target: MenuTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewBranch {
    pub name: String,
    /// The branch or commit the new one starts at, ready to show.
    pub from: String,
    pub checkout: bool,
    /// Why the name does not work, ready to show.
    pub problem: Option<String>,
}

/// What the user is asked to confirm; the job of [`Question::job`] runs when they do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Question {
    DiscardFile { path: String },
    DiscardAll { count: usize },
    DeleteBranch { name: String },
    DeleteOnRemote { remote: String, branch: String },
    DropStash { sha: String, message: String },
}

impl Question {
    /// Whether "undo" brings back what the action takes away.
    pub fn can_be_undone(&self) -> bool {
        match self {
            Self::DiscardFile { .. } | Self::DiscardAll { .. } | Self::DeleteBranch { .. } => true,
            Self::DeleteOnRemote { .. } | Self::DropStash { .. } => false,
        }
    }

    pub fn job(&self) -> Job {
        match self {
            Self::DiscardFile { path } => Job::DiscardFile(path.clone()),
            Self::DiscardAll { .. } => Job::DiscardAll,
            Self::DeleteBranch { name } => Job::DeleteBranch {
                name: name.clone(),
                force: false,
            },
            Self::DeleteOnRemote { remote, branch } => Job::DeleteOnRemote {
                remote: remote.clone(),
                branch: branch.clone(),
            },
            Self::DropStash { sha, .. } => Job::StashDrop(sha.clone()),
        }
    }
}

/// The dialog for a new name of a branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameBranch {
    pub from: String,
    pub name: String,
}

/// A dialog over the tab, besides the one for a new branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dialog {
    Ask(Question),
    Rename(RenameBranch),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub path: String,
    /// The number of places in the file.
    pub places: u32,
    /// Changed in the editor, but not marked as resolved yet.
    pub edited: bool,
    pub resolved: bool,
}

/// Which side of a conflict is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Mine,
    Theirs,
    Both,
}

/// What can be done with a whole file in conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictAction {
    Take(Side),
    OpenEditor,
    MarkResolved,
    Reset,
}

/// A line of a file with conflict markers, as the diff shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictLine {
    Shared(u32, String),
    Mine(String),
    Theirs(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeInProgress {
    pub from: String,
    pub into: String,
    pub conflicts: Vec<Conflict>,
    pub selected: usize,
    /// The file of `selected`, with its markers.
    pub preview: Vec<ConflictLine>,
}

impl MergeInProgress {
    pub fn open_count(&self) -> usize {
        self.conflicts.iter().filter(|c| !c.resolved).count()
    }
}

/// A push or pull that did not work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// The remote has commits the local branch lacks.
    PushRejected { branch: String, behind: u32 },
    /// The remote did not let us in.
    AuthFailed {
        remote: String,
        /// What git printed, shown when the user asks for the details.
        output: String,
        details_open: bool,
    },
    /// The remote could not be reached.
    NetworkFailed { output: String, details_open: bool },
    /// A branch with commits no other branch has; deleting it needs a second go.
    BranchNotMerged { branch: String },
    /// A pull on diverged branches: merge or rebase, the user decides.
    NeedsPullStrategy,
    NothingToCommit,
    CommitTitleMissing,
    UnresolvedConflicts { count: usize },
    UndoNotSafe,
    InvalidName { name: String },
    /// Anything else git reported.
    Other { output: String, details_open: bool },
}

/// What a button of the toolbar is busy with; further presses wait for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Stage,
    Unstage,
    Discard,
    Commit,
    Fetch,
    Pull,
    Push,
    Stash,
    Undo,
    Branch,
    Integrate,
    Conflict,
}

/// Why the actions are not offered: the tab stays read only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unavailable {
    GitMissing,
    Sandbox,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StashEntry {
    pub message: String,
    /// The branch it was made on and how long ago, ready to show.
    pub origin: String,
}

#[derive(Debug, Default)]
pub struct Actions {
    pub local: Option<LocalChanges>,
    pub sync: Option<SyncBar>,
    pub menu: Option<OpenMenu>,
    pub new_branch: Option<NewBranch>,
    pub dialog: Option<Dialog>,
    pub merge: Option<MergeInProgress>,
    pub notice: Option<Notice>,
    pub stashes: Vec<StashEntry>,
    /// "Local changes" is chosen in the left list: the middle shows the diff of
    /// the chosen file and the right side the files and the commit message.
    pub changes_open: bool,
    /// The operation running off-thread; presses wait for it.
    pub busy: Option<Operation>,
    /// Set when git is missing or the build runs no git actions.
    pub unavailable: Option<Unavailable>,
    /// How to take back the last undoable action.
    pub undo: Option<UndoRecord>,
    /// A rebase is in progress: "mine" is git's "theirs".
    pub rebasing: bool,
    /// The stash commits in the order of `stashes`.
    pub stash_ids: Vec<String>,
    /// What "try again" repeats.
    pub retry: Option<Job>,
    /// Every file that was in conflict during this merge, resolved ones too.
    pub seen_conflicts: Vec<String>,
}
