use iced::mouse::ScrollDelta;

use crate::core::{Compared, Direction};
use crate::scan::ScanError;

/// Everything the start dialog, the bar and the sync dialog send.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// The key action and the command: opens the start dialog.
    Open,
    ToggleSize,
    ToggleDate,
    ToggleContent,
    ToggleSubfolders,
    /// Enter of a dialog: starts the comparison, or the sync.
    Confirm,
    /// Esc of a dialog.
    Cancel,
    /// A comparison ended; `generation` tells which one.
    Finished {
        generation: u64,
        result: Result<Compared, ScanError>,
    },
    ToggleOnlyDifferences,
    Again,
    /// The bar's button: ends the comparison.
    Done,
    OpenSync(Direction),
    ToggleDelete,
    SyncScrolled {
        deletions: bool,
        delta: ScrollDelta,
    },
    /// The panels moved, maybe out of the compared folders.
    PanelsChanged,
    /// The copy queue and the delete dialog have nothing left to do.
    OpsIdle,
}
