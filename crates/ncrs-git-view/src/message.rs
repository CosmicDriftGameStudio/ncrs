//! What happens in the Git tab.

use ncrs_git::{ChangedFile, CommitDetail, FileDiff, Log, RefsOverview};

use crate::state::Section;

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
    ToggleSection(Section),
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
