//! The messages of the file operations.

use ncrs_fs::{RestoreError, TrashedItem};

use crate::transfer::{Policy, Scan};
use ncrs_core::TransferKind;

/// What the dialog and its background work send the app.
#[derive(Debug, Clone, PartialEq)]
pub enum TransferDialogMessage {
    /// The scan of the selection is done. `None`: the thread died.
    Scanned {
        dialog: u64,
        scan: Option<Scan>,
    },
    /// The names the target already has, for the target as it was typed at
    /// edit number `edit`; a count for an older edit is dropped.
    ConflictsCounted {
        dialog: u64,
        edit: u64,
        count: usize,
    },
    TargetInput(String),
    SetKind(TransferKind),
    /// Tab: the other one of copy and move.
    ToggleKind,
    SetPolicy(Policy),
    ToggleList,
    Confirm,
    Cancel,
    /// Closes the progress mask; the job goes on.
    Background,
    /// Opens the progress mask again, from the status bar.
    ShowProgress,
}

/// Everything the trash view and putting entries back send.
#[derive(Debug, Clone, PartialEq)]
pub enum TrashMessage {
    /// Show the trash in place of the active panel.
    Open,
    /// What reading the trash gave. Dropped when `generation` is not the
    /// latest read's.
    Listed {
        generation: u64,
        result: Result<Vec<TrashedItem>, String>,
    },
    Close,
    Move(isize),
    Page(isize),
    First,
    Last,
    ToggleTag,
    TagMove(isize),
    TagAll,
    ClearTags,
    RowClicked(usize),
    Scrolled(iced::mouse::ScrollDelta),
    /// Put back the tagged rows, else the cursor row.
    Restore,
    RestoreRowDone {
        result: Result<(), RestoreError>,
        index: usize,
        generation: u64,
    },
    /// Put back what the latest F8 moved to the trash.
    RestoreLast,
    /// The entries of the trash that the latest deletion put there.
    LastListed {
        generation: u64,
        result: Result<Vec<TrashedItem>, String>,
    },
    OpenInFinder,
}

/// What to do about a name that is already taken.
///
/// The "All" options are the point: answering the same question per file turns
/// five files into five dialogs and fifty thousand into an afternoon.
///
/// The two `All` variants are what the single-file answers become when "for all
/// files" is ticked; see `App::answer_conflict`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictChoice {
    AllOverwrite,
    AllKeep,
    ThisOverwrite,
    ThisKeep,
    Cancel,
}

impl ConflictChoice {
    /// What to do with this one file.
    pub fn conflict(self) -> ncrs_fs::transfer::OnConflict {
        match self {
            ConflictChoice::AllOverwrite | ConflictChoice::ThisOverwrite => {
                ncrs_fs::transfer::OnConflict::Overwrite
            }
            // Cancel never reaches the filesystem; `Fail` is the rule that
            // would refuse rather than touch anything.
            ConflictChoice::AllKeep | ConflictChoice::ThisKeep => {
                ncrs_fs::transfer::OnConflict::Skip
            }
            ConflictChoice::Cancel => ncrs_fs::transfer::OnConflict::Fail,
        }
    }

    /// The same answer, applied to every later conflict too.
    pub fn for_all(self) -> Self {
        match self {
            ConflictChoice::ThisOverwrite => ConflictChoice::AllOverwrite,
            ConflictChoice::ThisKeep => ConflictChoice::AllKeep,
            other => other,
        }
    }

    /// Whether the answer also settles every later one.
    pub fn applies_to_all(self) -> bool {
        matches!(self, ConflictChoice::AllOverwrite | ConflictChoice::AllKeep)
    }
}

/// Everything the dialogs and the trash view of the file operations send, in
/// one enum: the app maps each into its own message.
#[derive(Debug, Clone, PartialEq)]
pub enum OpsMessage {
    /// The copy or move dialog, and the progress mask after it.
    TransferDialog(TransferDialogMessage),
    /// The user answered the conflict dialog.
    TransferConflict(ConflictChoice),
    /// Ticks or unticks "for all files" in the conflict dialog.
    ToggleConflictAll,
    DeleteCancel,
    DeleteConfirm,
    /// The progress mask's abort button.
    AbortJob,
    Trash(TrashMessage),
}
