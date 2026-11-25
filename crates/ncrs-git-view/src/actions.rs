//! What the Git tab shows of the actions of #338: local changes, the toolbar,
//! menus, the new-branch dialog, a merge with conflicts and notices. Design
//! phase: the state is filled by the caller, nothing here reads a repository.

use iced::widget::text_editor;
use ncrs_git::{ChangedFile, FileDiff, UndoRecord};

use crate::job::Job;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    /// "Push afterwards": the commit button commits and pushes.
    pub push_after: bool,
    pub focus: WipFocus,
    /// Enter was pressed on the file: the middle shows its diff instead of the graph.
    pub diff_open: bool,
    /// The diff in one column instead of old and new next to each other.
    pub unified: bool,
    /// The change (hunk) of the open diff that has the keys.
    pub hunk: usize,
    /// The diff does not count a change of white space. Its hunks are for
    /// looking only: the buttons of a hunk are locked while it is on.
    pub ignore_whitespace: bool,
    /// The lists of files that are folded shut.
    pub folded: std::collections::HashSet<ChangeSide>,
}

/// The stops of Tab inside the pane of the local changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WipFocus {
    #[default]
    Unstaged,
    Staged,
    Title,
    Body,
    Amend,
    PushAfter,
    Commit,
}

impl WipFocus {
    const RING: [Self; 7] = [
        Self::Unstaged,
        Self::Staged,
        Self::Title,
        Self::Body,
        Self::Amend,
        Self::PushAfter,
        Self::Commit,
    ];

    fn place(self) -> usize {
        Self::RING
            .iter()
            .position(|stop| *stop == self)
            .unwrap_or_default()
    }

    pub fn next(self) -> Self {
        Self::RING
            .get((self.place() + 1) % Self::RING.len())
            .copied()
            .unwrap_or_default()
    }

    pub fn previous(self) -> Self {
        Self::RING
            .get((self.place() + Self::RING.len() - 1) % Self::RING.len())
            .copied()
            .unwrap_or_default()
    }

    pub fn is_field(self) -> bool {
        matches!(self, Self::Title | Self::Body)
    }
}

/// What the user can put into `.gitignore` for one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IgnoreChoice {
    File,
    Folder,
    Extension,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuTarget {
    LocalBranch(usize),
    /// The index into the remote branches of the overview.
    RemoteBranch(usize),
    /// The id of the stash: its place in the list moves when stashes come and go.
    Stash(String),
    /// The index into the conflicts of the merge.
    Conflict(usize),
    /// The full SHA of the commit: its row moves when the list reloads.
    Commit(String),
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenMenu {
    pub target: MenuTarget,
    /// The entry of a commit menu the keys are on.
    pub cursor: usize,
    /// The entry of the open reset submenu the keys are on.
    pub submenu: Option<usize>,
}

impl OpenMenu {
    pub const fn new(target: MenuTarget) -> Self {
        Self {
            target,
            cursor: 0,
            submenu: None,
        }
    }
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
    DiscardFile {
        path: String,
    },
    DiscardAll {
        count: usize,
    },
    DeleteBranch {
        name: String,
    },
    DeleteOnRemote {
        remote: String,
        branch: String,
    },
    DropStash {
        sha: String,
        message: String,
    },
    /// Which pattern goes into `.gitignore`; only the offered choices are set.
    Ignore {
        path: String,
        folder: Option<String>,
        extension: Option<String>,
        chosen: IgnoreChoice,
    },
    /// A hard reset: what it would lose was read before the question.
    ResetHard {
        /// The branch, or the word for a detached HEAD, ready to show.
        branch: String,
        sha: String,
        commits: u32,
        changed_files: u32,
    },
}

impl Question {
    /// Whether "undo" brings back what the action takes away.
    pub fn can_be_undone(&self) -> bool {
        match self {
            Self::DiscardFile { .. }
            | Self::DiscardAll { .. }
            | Self::DeleteBranch { .. }
            | Self::Ignore { .. }
            | Self::ResetHard { .. } => true,
            Self::DeleteOnRemote { .. } | Self::DropStash { .. } => false,
        }
    }

    /// Enter presses the cancel button, not the one that does the harm.
    pub fn enter_is_safe(&self) -> bool {
        matches!(self, Self::ResetHard { .. })
    }

    pub fn job(&self) -> Job {
        match self {
            Self::ResetHard { sha, .. } => Job::Reset {
                mode: ncrs_git::ResetMode::Hard,
                sha: sha.clone(),
            },
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
            Self::Ignore {
                path,
                folder,
                extension,
                chosen,
            } => Job::Ignore {
                pattern: match (chosen, folder, extension) {
                    (IgnoreChoice::Folder, Some(directory), _) => {
                        format!("{}/", ncrs_git::literal_pattern(directory))
                    }
                    (IgnoreChoice::Extension, _, Some(ending)) => format!("*.{ending}"),
                    _ => ncrs_git::literal_pattern(path),
                },
            },
        }
    }
}

/// The dialog for a new name of a branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameBranch {
    pub from: String,
    pub name: String,
}

/// The dialog for the name of a tag on a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagDraft {
    pub name: String,
    pub sha: String,
    /// Why the name does not work, ready to show.
    pub problem: Option<String>,
}

/// A dialog over the tab, besides the one for a new branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dialog {
    Ask(Question),
    Rename(RenameBranch),
    Tag(TagDraft),
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
    pub kind: ncrs_git::Integration,
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
    PushRejected {
        branch: String,
        behind: u32,
    },
    /// The remote did not let us in.
    AuthFailed {
        remote: String,
        /// What git printed, shown when the user asks for the details.
        output: String,
        details_open: bool,
    },
    /// The remote could not be reached.
    NetworkFailed {
        output: String,
        details_open: bool,
    },
    /// A branch with commits no other branch has; deleting it needs a second go.
    BranchNotMerged {
        branch: String,
    },
    /// A pull on diverged branches: merge or rebase, the user decides.
    NeedsPullStrategy,
    NothingToCommit,
    CommitTitleMissing,
    UnresolvedConflicts {
        count: usize,
    },
    UndoNotSafe,
    InvalidName {
        name: String,
    },
    /// Anything else git reported.
    Other {
        output: String,
        details_open: bool,
    },
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
