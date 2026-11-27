use std::path::PathBuf;
use std::sync::Arc;

use ncrs_space::{Group, RunLog, Scan};

use crate::decide::DecideMessage;

/// What the menu of a row offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuEntry {
    ShowInPanel,
    DeleteOnlyThis,
    AskHermes,
}

/// How a delete run ended, as the worker reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    pub run: u64,
    pub log: Arc<RunLog>,
    /// The free bytes of the drive after the run; `None` when they could not be read.
    pub free_after: Option<u64>,
    /// Why the log file could not be written.
    pub log_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    // Keys of the list.
    Up,
    Down,
    PageUp,
    PageDown,
    First,
    Last,
    /// Folds the group of the cursor, or goes to its head.
    Left,
    /// Opens the group under the cursor.
    Right,
    /// Space: ticks or unticks the row.
    Toggle,
    Enter,
    Escape,
    FocusFilter,
    FilterChanged(String),
    FilterSubmitted,
    OpenMenuKey,
    // The actions.
    DeleteSafe,
    RemoveWorktrees,
    DeleteOnlyThis,
    Rescan,
    Retry,
    AskHermes,
    GrantHome,
    ShowInPanelAtCursor,
    // Mouse.
    RowPressed(PathBuf),
    RowToggled(PathBuf),
    HeadToggled(Group),
    OpenMenuAt(PathBuf),
    CloseMenu,
    MenuCursor(usize),
    MenuUp,
    MenuDown,
    MenuEnter,
    MenuPicked(MenuEntry),
    ListScrolled(f32),
    // The measure.
    ScanProgress {
        run: u64,
        bytes: u64,
    },
    Scanned {
        run: u64,
        scan: Option<Arc<Scan>>,
    },
    // The confirm.
    ConfirmCancel,
    ConfirmAccept,
    /// Enter in the dialog: does what the focus is on.
    ConfirmEnter,
    ConfirmSwitchFocus,
    ConfirmWithWorktreesToggled,
    // The run and its result.
    RunProgress {
        run: u64,
        done: usize,
    },
    RunFinished(Finished),
    CloseResult,
    ShowLog,
    HideLog,
    LogRead(Vec<String>),
    // The decide pane.
    Decide(DecideMessage),
}
