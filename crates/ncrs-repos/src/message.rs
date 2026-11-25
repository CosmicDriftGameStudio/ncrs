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
    MenuPicked(MenuEntry),
    // The filter.
    FilterChanged(String),
    FilterSubmitted,
    OnlyNeedingActionToggled,
    // The list.
    ScanNow,
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
    /// Every minute while the tab is shown: reads the visible repos again.
    Tick,
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
    // The clone dialog.
    CloneOpen,
    CloneUrlChanged(String),
    ClonePickTarget,
    CloneTargetPicked(PathBuf),
    CloneRun,
    CloneDone(Result<PathBuf, String>),
    CloneClose,
    // The clean-up pane.
    Cleanup(CleanupMessage),
}
