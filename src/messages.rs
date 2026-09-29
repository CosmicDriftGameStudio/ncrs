//! Central message type. Every state change in the app is triggered by one
//! of these messages and handled in [`crate::app::App::update`].

use std::path::PathBuf;

use iced::Size;

use crate::dialog::PromptKind;
use crate::fs::CreateDirError;
use crate::fs::{FileEntry, ReadError};

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

    /// A key press, unclassified. `iced`'s `on_key_press` only takes a plain
    /// `fn`, which cannot read app state, so the key arrives raw and
    /// `App::update` decides whether it belongs to the prompt or the panels.
    Typed {
        key: Box<iced::keyboard::Key>,
        modifiers: iced::keyboard::Modifiers,
    },

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

    // --- Panel handling ---
    SwitchPanel,
    /// Temporarily switches the UI language. Not yet persisted in config.
    SwitchLanguage,
    /// Mouse click on a row of a panel.
    RowClicked {
        side: PanelSide,
        index: usize,
    },

    // --- Window / app ---
    WindowResized(Size),
    Quit,
}
