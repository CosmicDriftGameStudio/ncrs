//! Central message type. Every state change in the app is triggered by one
//! of these messages and handled in [`crate::app::App::update`].

use std::path::PathBuf;

use iced::Size;

use crate::app::{RowFailure, Transfer};
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
    /// A printable key: jump to the first entry that starts with what was
    /// typed in the last second.
    TypeAhead(String),

    // --- copy / move (F5, F6) ---
    /// Ctrl+C or Escape while a job runs: stop it. Its own message rather than
    /// reusing a key binding, because "stop" only exists while a job does.
    AbortJob,
    /// One tick of a running transfer, sent from the blocking thread.
    JobProgress(crate::fs::transfer::Tick),
    /// F5 or F6, decided by the action rather than a message each.
    Transfer(TransferKind),
    /// Path or name of the tagged rows, else the cursor row, to the clipboard.
    CopyToClipboard(ClipboardKind),
    /// The user answered the conflict dialog.
    TransferConflict(ConflictChoice),
    /// Ticks or unticks "for all files" in the conflict dialog.
    ToggleConflictAll,
    /// One row of a transfer finished. Carries the whole transfer so the next
    /// row can start without the app keeping it in a field that a later
    /// message could overwrite.
    TransferRowDone {
        /// `Err` carries a typed [`RowFailure`], not a bare string: the app has
        /// to tell "the name is taken" from any other error, and a message that
        /// cannot be told apart is the reason the overwrite dialog was
        /// unreachable.
        result: Result<Transfer, RowFailure>,
        /// Which row this was, so a conflict on the next one points at it.
        index: usize,
        /// The transfer it belongs to; a result from an earlier one is dropped.
        generation: u64,
    },

    /// Left/Right/Tab in a button dialog: move the focus by one.
    DialogFocus(isize),
    /// Enter in a button dialog: press the focused button.
    DialogActivate,
    /// A modifier key went down or up.
    ModifiersChanged(iced::keyboard::Modifiers),

    // --- view / edit (F3, F4) ---
    /// F3 or F4: hand the file under the cursor to an external program.
    OpenExternal(crate::fs::OpenKind),

    // --- delete (F8, Shift+F8) ---
    /// F8 (`permanent: false`) or Shift+F8: open the confirmation dialog.
    Delete {
        permanent: bool,
    },
    /// The user confirmed the delete dialog.
    DeleteConfirm,
    /// The user dismissed the delete dialog.
    DeleteCancel,
    /// One entry of a delete finished.
    DeleteRowDone {
        /// The reason, as the OS or the trash worded it; the app adds the name.
        result: Result<(), String>,
        index: usize,
        /// The delete it belongs to; a result from an earlier one is dropped.
        generation: u64,
    },

    // --- drive menu (Alt+F1, Alt+F2) ---
    /// Alt+F1 or Alt+F2: choose the drive of that panel.
    VolumeMenu(PanelSide),
    /// Up or Down in the open menu.
    VolumeMenuMove(isize),
    VolumeMenuFirst,
    VolumeMenuLast,
    /// Enter in the open menu: go to the highlighted drive.
    VolumeMenuActivate,
    /// A click on the entry at this index: highlight it and go there.
    VolumeMenuClick(usize),
    VolumeMenuClose,

    // --- context menu (right click, Option tap, Shift+F10) ---
    /// Right click (or Ctrl+click on a Mac) on a row: open the menu at the pointer.
    ContextMenuAt {
        side: PanelSide,
        index: usize,
    },
    /// Shift+F10 or a tap of Option: open the menu at the cursor row.
    ContextMenuKey,
    ContextMenuMove(isize),
    ContextMenuFirst,
    ContextMenuLast,
    /// Enter in the open menu: run the highlighted entry.
    ContextMenuActivate,
    /// A click on the entry at this index: run it.
    ContextMenuClick(usize),
    /// The pointer is over the entry at this index.
    ContextMenuHover(usize),
    ContextMenuClose,

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
    /// Insert or Space: tag or untag the row under the cursor and go down one.
    ToggleTag,
    /// Shift+Up/Down: tag or untag the row under the cursor, then move.
    TagMove(isize),
    /// `*`: tag everything except `..`.
    TagAll,
    /// Ctrl+`*`: drop every tag.
    ClearTags,

    // --- Panel handling ---
    /// Wheel or trackpad over a panel.
    PanelScrolled {
        side: PanelSide,
        delta: iced::mouse::ScrollDelta,
    },
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

/// What the copy chords put on the clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardKind {
    Path,
    Name,
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
    pub fn conflict(self) -> crate::fs::transfer::OnConflict {
        match self {
            ConflictChoice::AllOverwrite | ConflictChoice::ThisOverwrite => {
                crate::fs::transfer::OnConflict::Overwrite
            }
            // Cancel never reaches the filesystem; `Fail` is the rule that
            // would refuse rather than touch anything.
            ConflictChoice::AllKeep | ConflictChoice::ThisKeep => {
                crate::fs::transfer::OnConflict::Skip
            }
            ConflictChoice::Cancel => crate::fs::transfer::OnConflict::Fail,
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
