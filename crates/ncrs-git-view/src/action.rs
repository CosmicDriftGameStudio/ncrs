//! What the Git tab asks of the app.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// The tab is to go.
    Close,
    /// Show this worktree in the active panel.
    ShowWorktree(PathBuf),
    /// Show this file in the viewer (F3).
    ViewFile(PathBuf),
}
