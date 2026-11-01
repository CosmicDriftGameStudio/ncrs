//! What the file operations ask the app to do, which they cannot do
//! themselves. The app runs it right after `update`.

use ncrs_fs::{RestoreError, TrashedItem};

/// What the copy and move dialog asks of the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferAction {
    /// The typed target changed: count its taken names again.
    CountConflicts,
    /// The dialog was closed without a transfer: give the keyboard focus back.
    Closed,
    /// Enter: check the target and start the transfer.
    Confirm,
    /// Close the progress mask; the job goes on.
    HideProgress,
    /// Open the progress mask again if a job runs.
    ShowProgress,
}

/// What the trash view asks of the app.
#[derive(Debug, Clone, PartialEq)]
pub enum TrashAction {
    /// Show the trash in place of the active panel.
    Open,
    Close,
    /// Put these entries back.
    Restore(Vec<TrashedItem>),
    RestoreRowDone {
        result: Result<(), RestoreError>,
        index: usize,
        generation: u64,
    },
    RestoreLast,
    LastListed {
        generation: u64,
        result: Result<Vec<TrashedItem>, String>,
    },
    OpenInFinder,
}
