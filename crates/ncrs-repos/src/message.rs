use std::path::PathBuf;

use crate::bulk::{BulkAction, Executed};
use crate::cleanup::CleanupMessage;
use crate::rows::GroupKey;
use crate::scan::ScanResult;
use crate::summary::Reading;

/// What the menu of a row or of the bar offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuEntry {
    Open,
    Favourite,
    Remove,
    Bulk(BulkAction),
    OpenInTabs,
    Cleanup,
    ShowInFiles,
    OpenTerminal,
    CopyPath,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    // Keys of the list.
    Up,
    Down,
    Left,
    /// Tab: moves the keys between the list and the clean-up pane.
    SwitchFocus,
    Right,
    ExtendUp,
    ExtendDown,
    PageUp,
    PageDown,
    First,
    Last,
    Mark,
    MarkAll,
    Enter,
    ToggleFavouriteAtCursor,
    RemoveAtCursor,
    FocusFilter,
    Escape,
    OpenMenuKey,
    ShowInFilesAtCursor,
    OpenTerminalAtCursor,
    CopyPathAtCursor,
    // Mouse.
    RowPressed(PathBuf),
    RowToggled(PathBuf),
    AllToggled,
    FavouriteClicked(PathBuf),
    RowOpened(PathBuf),
    RowRemoved(PathBuf),
    Hovered(Option<PathBuf>),
    GroupToggled(GroupKey),
    OpenMenuAt(PathBuf),
    CloseMenu,
    MenuCursor(usize),
    MenuUp,
    MenuDown,
    MenuEnter,
    MenuPicked(MenuEntry),
    // The filter.
    FilterChanged(String),
    FilterSubmitted,
    OnlyNeedingActionToggled,
    /// The switch "Show worktrees".
    ShowWorktreesToggled,
    // The list.
    /// Searches again, or stops the search that runs.
    ScanNow,
    /// What the search looks at may have changed: it starts anew.
    Rescan,
    Scanned(Box<ScanResult>),
    AddRepo,
    RepoPicked(PathBuf),
    /// A file panel opened a repository (the app sends its root).
    Learn(PathBuf),
    SummaryRead(PathBuf, Box<Reading>),
    /// How many branches and worktrees a repository could lose; none when it could not be read.
    CleanableRead(PathBuf, Option<usize>),
    /// The list was scrolled by the pointer; how far down, in pixels.
    ListScrolled(f32),
    /// The handle of a column head was pressed.
    ResizePressed(ncrs_ui_kit::repo_row::RepoColumn),
    /// The pointer moved while a column is dragged; its x.
    ResizeMoved(f32),
    ResizeReleased,
    /// Every minute while the tab is in view: reads every repo again.
    Tick,
    /// The tab came into view: reads every repo again when the last read is old.
    Shown,
    // Bulk actions.
    Bulk(BulkAction),
    StepDone(PathBuf, Box<Executed>),
    RetryFailed,
    OnlyProblemsToggled,
    CloseResult,
    OpenInTabs,
    ShowInGitTab(PathBuf),
    ShowDetails(PathBuf),
    ClearDetails,
    // The login dialog of a push.
    CredentialAsked(ncrs_git::askpass::AskRequest),
    CredentialTyped(u64, ncrs_git::askpass::Secret),
    CredentialSend(u64),
    CredentialCancel(u64),
    // The clone dialog.
    CloneOpen,
    CloneUrlChanged(String),
    ClonePickTarget,
    CloneTargetPicked(PathBuf),
    CloneRun,
    CloneDone(Result<PathBuf, String>),
    CloneClose,
    /// The disk warning in the status bar was pressed.
    AskWhyDiskFull,
    // The clean-up pane.
    Cleanup(CleanupMessage),
}
