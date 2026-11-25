//! What the Repos tab asks of the app.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Show this repository in a Git tab (a new one, or the one that has it).
    OpenGitTab(PathBuf),
    /// Show each of these in a Git tab.
    OpenGitTabs(Vec<PathBuf>),
    /// Ask for a folder; the answer comes back as `Message::RepoPicked`.
    PickFolder,
    /// Ask for the folder a clone goes into; the answer comes back as `Message::CloneTargetPicked`.
    PickCloneTarget,
    /// A worktree folder was deleted: close the Git tab that shows it.
    WorktreeRemoved(PathBuf),
    /// Branches of this repository changed: a Git tab that shows it reads it again.
    RepoChanged(PathBuf),
}
