//! What the actions ask of git: one blocking call each, run on the blocking pool.

use std::path::Path;

use ncrs_git::{
    ActionError, CommitMessage, ConflictChoice, DiffHunk, Git, PullStrategy, UndoRecord,
};

use crate::actions::Operation;

/// One call to git, with everything it needs, so it can run off the UI thread
/// and be repeated.
#[derive(Debug, Clone, PartialEq)]
pub enum Job {
    Stage(Vec<String>),
    StageAll,
    Unstage(Vec<String>),
    UnstageAll,
    StageHunk {
        path: String,
        hunk: DiffHunk,
    },
    UnstageHunk {
        path: String,
        hunk: DiffHunk,
    },
    DiscardHunk {
        path: String,
        hunk: DiffHunk,
    },
    DiscardFile(String),
    DiscardAll,
    Commit {
        message: CommitMessage,
        amend: bool,
    },
    /// Commits, then pushes; a push that fails leaves the commit.
    CommitAndPush {
        message: CommitMessage,
        amend: bool,
    },
    Ignore {
        pattern: String,
    },
    Fetch,
    Pull(Option<PullStrategy>),
    Push,
    PushBranch(String),
    StashPush {
        message: String,
    },
    StashApply(String),
    StashPop(String),
    Undo(UndoRecord),
    CreateBranch {
        name: String,
        from: String,
        checkout: bool,
    },
    Checkout(String),
    CheckoutRemote(String),
    /// Puts the local changes on the stash, then switches to `branch`; `remote` for a remote branch.
    StashAndCheckout {
        message: String,
        branch: String,
        remote: bool,
    },
    DeleteBranch {
        name: String,
        force: bool,
    },
    RenameBranch {
        from: String,
        to: String,
    },
    DeleteOnRemote {
        remote: String,
        branch: String,
    },
    StashDrop(String),
    Merge(String),
    Rebase(String),
    ResolveBlock {
        path: String,
        block: usize,
        choice: ConflictChoice,
    },
    ResolveFile {
        path: String,
        choice: ConflictChoice,
    },
    MarkResolved(String),
    ResetFile(String),
    AbortIntegration,
    ContinueIntegration,
}

/// What a job that worked leaves for the state.
#[derive(Debug, Clone, PartialEq)]
pub enum Done {
    Plain,
    /// The way back, if the action has one.
    Undoable(Option<UndoRecord>),
    Undone,
    /// Commit and push: the commit is made (and undoable), the push failed.
    CommittedButNotPushed {
        undo: Option<UndoRecord>,
        error: ActionError,
    },
}

/// A job and what came of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Finished {
    pub job: Job,
    pub result: Result<Done, ActionError>,
}

fn borrowed(paths: &[String]) -> Vec<&str> {
    paths.iter().map(String::as_str).collect()
}

/// A file whose last block was just chosen has no markers left: it counts as
/// resolved without a second press.
fn mark_when_clean(git: &Git, path: &str) -> Result<(), ActionError> {
    let clean = git
        .conflicts()?
        .iter()
        .any(|conflict| conflict.path == path && conflict.blocks.is_empty());
    if clean {
        git.mark_resolved(path)?;
    }
    Ok(())
}

impl Job {
    pub fn operation(&self) -> Operation {
        match self {
            Self::Stage(_) | Self::StageAll | Self::StageHunk { .. } => Operation::Stage,
            Self::Unstage(_) | Self::UnstageAll | Self::UnstageHunk { .. } => Operation::Unstage,
            Self::DiscardHunk { .. } | Self::DiscardFile(_) | Self::DiscardAll => {
                Operation::Discard
            }
            Self::Commit { .. } | Self::CommitAndPush { .. } => Operation::Commit,
            Self::Ignore { .. } => Operation::Stage,
            Self::Fetch => Operation::Fetch,
            Self::Pull(_) => Operation::Pull,
            Self::Push | Self::PushBranch(_) => Operation::Push,
            Self::StashPush { .. }
            | Self::StashApply(_)
            | Self::StashPop(_)
            | Self::StashDrop(_) => Operation::Stash,
            Self::Undo(_) => Operation::Undo,
            Self::CreateBranch { .. }
            | Self::Checkout(_)
            | Self::CheckoutRemote(_)
            | Self::StashAndCheckout { .. }
            | Self::DeleteBranch { .. }
            | Self::RenameBranch { .. }
            | Self::DeleteOnRemote { .. } => Operation::Branch,
            Self::Merge(_)
            | Self::Rebase(_)
            | Self::AbortIntegration
            | Self::ContinueIntegration => Operation::Integrate,
            Self::ResolveBlock { .. }
            | Self::ResolveFile { .. }
            | Self::MarkResolved(_)
            | Self::ResetFile(_) => Operation::Conflict,
        }
    }

    pub fn run(&self, git: &Git) -> Result<Done, ActionError> {
        match self {
            Self::Stage(paths) => git.stage(&borrowed(paths)).map(Done::Undoable),
            Self::StageAll => git.stage_all().map(Done::Undoable),
            Self::Unstage(paths) => git.unstage(&borrowed(paths)).map(Done::Undoable),
            Self::UnstageAll => git.unstage_all().map(Done::Undoable),
            Self::StageHunk { path, hunk } => git.stage_hunk(path, hunk).map(Done::Undoable),
            Self::UnstageHunk { path, hunk } => git.unstage_hunk(path, hunk).map(Done::Undoable),
            Self::DiscardHunk { path, hunk } => git
                .discard_hunk(path, hunk)
                .map(|record| Done::Undoable(Some(record))),
            Self::DiscardFile(path) => git
                .discard_file(path)
                .map(|record| Done::Undoable(Some(record))),
            Self::DiscardAll => git.discard_all().map(|record| Done::Undoable(Some(record))),
            Self::Commit {
                message,
                amend: true,
            } => {
                let changed = (!message.title.trim().is_empty()).then_some(message);
                git.amend(changed).map(|()| Done::Plain)
            }
            Self::Commit {
                message,
                amend: false,
            } => git.commit(message).map(Done::Undoable),
            Self::CommitAndPush { message, amend } => {
                let committed = Self::Commit {
                    message: message.clone(),
                    amend: *amend,
                }
                .run(git)?;
                match git.push() {
                    Ok(()) => Ok(committed),
                    Err(error) => Ok(Done::CommittedButNotPushed {
                        undo: match committed {
                            Done::Undoable(record) => record,
                            _ => None,
                        },
                        error,
                    }),
                }
            }
            Self::Ignore { pattern } => git.ignore_pattern(pattern).map(Done::Undoable),
            Self::Fetch => git.fetch(None).map(|()| Done::Plain),
            Self::Pull(strategy) => git.pull(*strategy).map(|_| Done::Plain),
            Self::Push => git.push().map(|()| Done::Plain),
            Self::PushBranch(name) => git.push_branch(name, None).map(|()| Done::Plain),
            Self::StashPush { message } => git.stash_push(message, true).map(|_| Done::Plain),
            Self::StashApply(sha) => git.stash_apply(sha).map(|_| Done::Plain),
            Self::StashPop(sha) => git.stash_pop(sha).map(|_| Done::Plain),
            Self::Undo(record) => git.undo(record).map(|()| Done::Undone),
            Self::CreateBranch {
                name,
                from,
                checkout,
            } => git
                .create_branch(name, from, *checkout)
                .map(|()| Done::Plain),
            Self::Checkout(name) => git.checkout(name).map(|()| Done::Plain),
            Self::CheckoutRemote(name) => git.checkout_remote_branch(name).map(|()| Done::Plain),
            Self::StashAndCheckout {
                message,
                branch,
                remote,
            } => {
                git.stash_push(message, true)?;
                if *remote {
                    git.checkout_remote_branch(branch)
                } else {
                    git.checkout(branch)
                }
                .map(|()| Done::Plain)
            }
            Self::DeleteBranch { name, force } => git
                .delete_branch(name, *force)
                .map(|record| Done::Undoable(Some(record))),
            Self::RenameBranch { from, to } => git.rename_branch(from, to).map(|()| Done::Plain),
            Self::DeleteOnRemote { remote, branch } => git
                .delete_remote_branch(remote, branch)
                .map(|()| Done::Plain),
            Self::StashDrop(sha) => git.stash_drop(sha).map(|()| Done::Plain),
            Self::Merge(name) => git.merge(name).map(|_| Done::Plain),
            Self::Rebase(name) => git.rebase(name).map(|_| Done::Plain),
            Self::ResolveBlock {
                path,
                block,
                choice,
            } => {
                git.resolve_block(path, *block, *choice)?;
                mark_when_clean(git, path).map(|()| Done::Plain)
            }
            Self::ResolveFile { path, choice } => {
                git.resolve_file(path, *choice)?;
                mark_when_clean(git, path).map(|()| Done::Plain)
            }
            Self::MarkResolved(path) => git.mark_resolved(path).map(|()| Done::Plain),
            Self::ResetFile(path) => git.reset_file(path).map(|()| Done::Plain),
            Self::AbortIntegration => git.abort_integration().map(|()| Done::Plain),
            Self::ContinueIntegration => git.continue_integration().map(|_| Done::Plain),
        }
    }

    /// Opens the repository at `root` and runs the job there.
    pub fn run_in(self, root: &Path) -> Finished {
        let result = Git::open(root).and_then(|git| self.run(&git));
        Finished { job: self, result }
    }
}
