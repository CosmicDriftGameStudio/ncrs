//! What happens in the Git tab.

use ncrs_git::{ChangedFile, CommitDetail, FileDiff, Log, RefsOverview};

use ncrs_ui_kit::git_rows::CommitColumn;

use crate::state::Fold;

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// The answers of the blocking reads, with the number of the read they belong to.
    RefsLoaded(u64, Box<Result<RefsOverview, String>>),
    LogLoaded(u64, Box<Result<Log, String>>),
    DetailLoaded(u64, Box<Result<CommitDetail, String>>),
    WorkingLoaded(u64, Result<Vec<ChangedFile>, String>),
    DiffLoaded(u64, Box<Result<FileDiff, String>>),
    FilterChanged(String),
    FocusFilter,
    /// Enter in the filter field: back to the list.
    FilterSubmit,
    SidebarPressed(usize),
    CommitPressed(usize),
    FilePressed(usize),
    /// A folder row of the file tree: the pane gets the focus.
    FolderPressed,
    ToggleFold(Fold),
    /// The gap before a commit column was pressed: dragging changes its width.
    ResizePressed(CommitColumn),
    /// The pointer moved to this x while a column is dragged.
    ResizeMoved(f32),
    ResizeReleased,
    LoadMore,
    SetTreeMode(bool),
    SetUnified(bool),
    ToggleIgnoreWhitespace,
    Up,
    Down,
    Left,
    Right,
    Enter,
    NextArea,
    PreviousArea,
    NextHunk,
    PreviousHunk,
    NextFile,
    PreviousFile,
    /// F3: show the file of the diff in the viewer.
    ViewFile,
    /// Escape: leaves the filter, then the diff, then the tab.
    Escape,
    Close,
    Refresh,
}
