//! What the Git tab asks of the app.

use std::path::PathBuf;

use ncrs_ui_kit::git_rows::CommitColumns;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// The tab is to go.
    Close,
    /// Show the repository of this worktree in a Git tab of its own.
    OpenWorktreeTab(PathBuf),
    /// Show this file in the viewer (F3).
    ViewFile(PathBuf),
    /// Open this file in the editor (F4).
    OpenInEditor(PathBuf),
    /// The groups folded in the left list of the repository at `root` changed;
    /// the app remembers them.
    FoldsChanged { root: PathBuf, folded: Vec<String> },
    /// The commit columns were dragged to these widths; the app remembers them.
    ColumnsResized(CommitColumns),
}
