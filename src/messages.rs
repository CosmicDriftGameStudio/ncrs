//! Central message type. Every state change in the app is triggered by one
//! of these messages and handled in [`crate::app::App::update`].

use std::path::PathBuf;

use iced::Size;

use crate::app::Transfer;
use crate::dialog::PromptKind;
use crate::fs::CreateDirError;
use crate::fs::{FileEntry, ReadError};
use crate::jobs::JobEvent;

/// Identifies one of the two file panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelSide {
    Left,
    Right,
}

impl PanelSide {
    pub fn other(self) -> Self {
        match self {
            PanelSide::Left => PanelSide::Right,
            PanelSide::Right => PanelSide::Left,
        }
    }
}

/// PartialEq so bindings can be compared; Eq is not derivable because
/// WindowResized carries floats.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// Result of an async directory read.
    DirectoryLoaded {
        side: PanelSide,
        /// Matches `PanelState::request_id`; stale results are dropped.
        request_id: u64,
        path: PathBuf,
        entries: Vec<FileEntry>,
        error: Option<ReadError>,
        /// Entry name to select after loading (e.g. the dir we came from).
        select: Option<String>,
    },

    // --- Navigation inside the active panel ---
    MoveSelection(isize),
    PageUp,
    PageDown,
    SelectFirst,
    SelectLast,
    OpenSelected,
    GoUp,

    // --- copy / move (F5, F6) ---
    /// Ctrl+C or Escape while a job runs: stop it. Its own message rather than
    /// reusing a key binding, because "stop" only exists while a job does.
    AbortJob,
    /// F5 or F6, decided by the action rather than a message each.
    Transfer(TransferKind),
    /// The user answered the conflict dialog.
    ///
    /// Nothing sends this yet: a taken name fails the row instead of asking.
    /// The dialog is the next piece; the message and the plumbing are here so
    /// it is one step, not a rewrite.
    #[allow(dead_code)]
    TransferConflict(ConflictChoice),
    /// One row of a transfer finished. Carries the whole transfer so the next
    /// row can start without the app keeping it in a field that a later
    /// message could overwrite.
    TransferRowDone {
        result: Result<Transfer, String>,
        /// Which row this was, so a conflict on the next one points at it.
        index: usize,
    },

    // --- Job queue (see crate::jobs) ---
    /// A job started, made progress, or finished. One variant for all three:
    /// they are the same event arriving at different times, and splitting them
    /// would mean the UI matching on the same enum in three places.
    JobFinished(JobEvent),

    // --- Modal prompt (see crate::dialog) ---
    /// F7: open the create-directory prompt.
    CreateDirPrompt,
    /// A keystroke went to the open prompt.
    PromptInput(String),
    /// The user confirmed the prompt.
    PromptSubmit,
    /// The user dismissed the prompt.
    PromptCancel,
    /// The background task finished.
    PromptFinished {
        /// The operation, so a stale result can be matched to its prompt.
        prompt: PromptKind,
        request_id: u64,
        result: Result<(), CreateDirError>,
    },

    // --- Selection ---
    /// Insert: tag or untag the row under the cursor.
    ToggleTag,
    /// `*`: tag everything except `..`.
    TagAll,
    /// Ctrl+`*`: drop every tag.
    ClearTags,

    // --- Panel handling ---
    SwitchPanel,
    /// Temporarily switches the UI language. Not yet persisted in config.
    SwitchLanguage,
    /// Mouse click on a row of a panel.  is true when the click landed in
    /// the tag column, which toggles the tag instead of moving the cursor.
    RowClicked {
        side: PanelSide,
        index: usize,
        on_tag: bool,
    },

    // --- Window / app ---
    WindowResized(Size),
    Quit,
}

/// Copy or move. One message with the kind inside, so the keymap and the job
/// queue do not each need a variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferKind {
    Copy,
    Move,
}

impl TransferKind {
    pub fn job_kind(self) -> crate::jobs::JobKind {
        match self {
            TransferKind::Copy => crate::jobs::JobKind::Copy,
            TransferKind::Move => crate::jobs::JobKind::Move,
        }
    }
}

/// What to do about a name that is already taken.
///
/// The "All" options are the point: answering the same question per file turns
/// five files into five dialogs and fifty thousand into an afternoon.
///
/// Not constructed yet — nothing sends `TransferConflict`. The dialog is the
/// next piece; the type and the resume path exist so it is one step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ConflictChoice {
    AllOverwrite,
    AllKeep,
    ThisOverwrite,
    ThisKeep,
    Cancel,
}

impl ConflictChoice {
    /// What to do with this one file.
    pub fn conflict(self) -> crate::fs::transfer::OnConflict {
        match self {
            ConflictChoice::AllOverwrite | ConflictChoice::ThisOverwrite => {
                crate::fs::transfer::OnConflict::Overwrite
            }
            // Keeping is "stop with an error", which the caller turns into the
            // next question rather than into a failure.
            ConflictChoice::AllKeep | ConflictChoice::ThisKeep | ConflictChoice::Cancel => {
                crate::fs::transfer::OnConflict::Fail
            }
        }
    }

    /// Whether the answer also settles every later one.
    #[allow(dead_code)]
    pub fn applies_to_all(self) -> bool {
        matches!(self, ConflictChoice::AllOverwrite | ConflictChoice::AllKeep)
    }
}
