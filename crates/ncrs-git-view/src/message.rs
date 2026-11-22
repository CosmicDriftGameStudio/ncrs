//! What happens in the Git tab.

use ncrs_git::{ChangedFile, CommitDetail, FileDiff, Log, RefsOverview};

use ncrs_ui_kit::git_rows::CommitColumn;

use crate::actions::{ChangeSide, ConflictAction, MenuAction, MenuTarget, Side};
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
    // The actions of #338. The handlers do nothing yet.
    SelectChange(ChangeSide, usize),
    Stage(usize),
    Unstage(usize),
    StageAll,
    UnstageAll,
    StageHunk(usize),
    DiscardHunk(usize),
    UnstageHunk(usize),
    DiscardFile(usize),
    DiscardAll,
    CommitTitleChanged(String),
    CommitBodyEdited(iced::widget::text_editor::Action),
    ToggleAmend,
    Commit,
    Fetch,
    Pull,
    Push,
    Stash,
    Undo,
    /// A right press on a row of the left list.
    OpenMenu(MenuTarget),
    /// The menu key: opens the menu of the row the cursor is on.
    OpenMenuKey,
    CloseMenu,
    MenuPressed(MenuAction),
    NewBranchNameChanged(String),
    ToggleNewBranchCheckout,
    CreateBranch,
    CloseDialog,
    MergeAbort,
    MergeContinue,
    SelectConflict(usize),
    /// A choice for one block of the chosen file: the number of the block.
    TakeBlock(usize, Side),
    /// An action of the menu of a file in conflict.
    ConflictPressed(usize, ConflictAction),
    /// The key of an action: it acts on the chosen file.
    ConflictShortcut(ConflictAction),
    PullFirst,
    Retry,
    ToggleNoticeDetails,
    DismissNotice,
}
