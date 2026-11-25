//! What happens in the Git tab.

use ncrs_git::{
    ActionError, Blame, ChangedFile, CommitDetail, FileDiff, FileHistory, Log, PullStrategy,
    RefLabel, RefsOverview, ResetLoss,
};

use crate::commit_menu::CommitAction;
use crate::file_views::FileAction;
use crate::state::Subject;

use ncrs_ui_kit::git_rows::CommitColumn;
use ncrs_ui_kit::splitter::{Direction, Step};

use crate::actions::{
    ChangeSide, ConflictAction, IgnoreChoice, MenuAction, MenuTarget, Side, WipFocus,
};
use crate::job::Finished;
use crate::panes::Split;
use crate::picker::PickerKind;
use crate::snapshot::Snapshot;
use crate::state::Fold;

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// The answers of the blocking reads, with the number of the read they belong to.
    RefsLoaded(u64, Box<Result<RefsOverview, String>>),
    LogLoaded(u64, Box<Result<Log, String>>),
    DetailLoaded(u64, Box<Result<CommitDetail, String>>),
    WorkingLoaded(u64, Result<Vec<ChangedFile>, String>),
    DiffLoaded(u64, Box<Result<FileDiff, String>>),
    /// The branches that contain the shown commit; they come after the detail.
    ContainingLoaded(u64, String, Box<Result<Vec<RefLabel>, String>>),
    /// What the actions show, read anew; the error says why there are none.
    ActionsLoaded(u64, Box<Result<Snapshot, ActionError>>),
    /// A call to git that was started by an action has ended.
    JobDone(Box<Finished>),
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
    // The actions of #338.
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
    /// Folds a list of the local changes shut or open.
    ToggleWipSection(ChangeSide),
    Commit,
    /// Commit, then push (the key, or the commit button while "push afterwards" is on).
    CommitAndPush,
    TogglePushAfter,
    /// Space: stages or unstages the chosen file, or presses the stop Tab is on.
    ToggleStageSelected,
    /// The key that jumps to the commit title.
    FocusCommitTitle,
    /// A press on a stop of the pane of the local changes.
    WipFocused(WipFocus),
    /// The pointer pressed in the title or the body: the field has the focus.
    WipFieldPressed(WipFocus),
    /// "Ignore…" on an unstaged file, by its path; the key asks for the chosen file.
    IgnoreFile(String),
    IgnoreSelected,
    IgnoreChoiceChanged(IgnoreChoice),
    /// F4 on a change: opens the chosen file in the editor.
    OpenSelectedInEditor,
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
    /// Enter or the confirm button of a question or the rename dialog.
    ConfirmDialog,
    RenameNameChanged(String),
    MergeAbort,
    MergeContinue,
    SelectConflict(usize),
    /// A choice for one block of the chosen file: the number of the block.
    TakeBlock(usize, Side),
    /// An action of the menu of a file in conflict.
    ConflictPressed(String, ConflictAction),
    /// The key of an action: it acts on the chosen file.
    ConflictShortcut(ConflictAction),
    PullFirst,
    /// The answer of the notice "pull needs a decision".
    PullWith(PullStrategy),
    /// The answer of the notice "branch not merged".
    DeleteBranchAnyway,
    Retry,
    ToggleNoticeDetails,
    DismissNotice,
    /// Opens the list of the repository or branch picker; a second press closes it.
    OpenPicker(PickerKind),
    PickerQuery(String),
    /// A press on a row of the open picker: the place among all its rows.
    PickerPressed(usize),
    /// Enter with the key for "and stash": switch the branch and put the local changes aside.
    PickerEnterWithStash,
    /// The key for a new branch while a picker is open.
    PickerNewBranch,
    /// The enter of the search field; the key handler has taken the key already.
    PickerSubmit,
    SplitPressed(Split),
    SplitHover(Split),
    SplitLeave,
    SplitReset(Split),
    SplitNudge(Split, Step, Direction),
    /// The pointer moved to this x while a splitter is dragged.
    PaneMoved(f32),
    PaneReleased,
    JumpToHead,
    JumpToWip,
    JumpToCommit(String),
    GoToParent,
    CopySha,
    /// The key that switches the file list between paths and a tree.
    ToggleTreeKey,
    ChipPressed(Chip),
    /// An entry of the commit menu, or its key: it acts on the commit of the open menu or the cursor.
    CommitAction(CommitAction),
    /// The pointer is over an entry of the commit menu.
    MenuCursor(usize),
    /// The pointer is over an entry of the reset submenu.
    MenuSubmenuCursor(usize),
    /// Space in the commit list: marks the commit to compare with, or lifts the mark.
    MarkCommit,
    /// The user scrolled the commit list this far down.
    CommitsScrolled(f32),
    TagNameChanged(String),
    CreateTag,
    /// What a hard reset to the commit would lose.
    ResetLossLoaded(String, Box<Result<ResetLoss, ActionError>>),
    /// The files of a comparison: the generation of the read, what is compared.
    CompareLoaded(u64, Subject, Box<Result<Vec<ChangedFile>, String>>),
    /// The commits that changed a file, from its path in the repository.
    OpenHistory(String),
    /// Who last changed each line of a file, at a commit or (`None`) the last one.
    OpenBlame(String, Option<String>),
    /// The keys of both: they mean the file the user is at.
    HistoryKey,
    BlameKey,
    HistoryLoaded(u64, Box<Result<FileHistory, String>>),
    HistoryDiffLoaded(u64, Box<Result<FileDiff, String>>),
    BlameLoaded(u64, Box<Result<Blame, String>>),
    HistoryPressed(usize),
    HistoryMore,
    HistoryScrolled(f32),
    BlamePressed(usize),
    BlameScrolled(f32),
    /// An entry of the menu of a file: it acts on the file of the open menu.
    FileMenu(FileAction),
}

/// The state chips of the head that jump somewhere when pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chip {
    Behind,
    Ahead,
    Changes,
    Conflicts,
}
