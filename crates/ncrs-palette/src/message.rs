//! What the command line, Quick Open and the command list send.

/// Everything Quick Open sends.
#[derive(Debug, Clone, PartialEq)]
pub enum QuickOpenMessage {
    Open,
    QueryChanged(String),
    /// A batch of the file walk. Dropped when `generation` is not the current
    /// walk's.
    Events {
        generation: u64,
        events: Vec<crate::quick_open::WalkEvent>,
    },
    /// The background ranking of `chunks_scored` chunks for the query of
    /// `generation` in the box of `walk_generation` is done.
    Ranked {
        walk_generation: u64,
        generation: u64,
        hits: Vec<crate::quick_open::Hit>,
        chunks_scored: usize,
    },
    /// The full-text search for the text of `generation` in the box of
    /// `walk_generation` is done. An answer of an older text is dropped.
    DocumentsFound {
        walk_generation: u64,
        generation: u64,
        result: Result<Vec<ncrs_core::FoundDocument>, String>,
    },
    Move(isize),
    Activate,
    RowActivated(usize),
    Close,
}

/// Everything the command line below the panels sends.
#[derive(Debug, Clone, PartialEq)]
pub enum CommandLineMessage {
    Focus,
    Leave,
    Input(String),
    Submit,
    HistoryPrevious,
    HistoryNext,
    /// The background look at the directory a `cd` asked for. An answer whose
    /// `request_id` is not the latest is dropped.
    CdChecked {
        request_id: u64,
        side: ncrs_core::PanelSide,
        target: std::path::PathBuf,
        result: Result<ncrs_fs::Destination, ncrs_fs::PathProblem>,
    },
    /// A shell command ended.
    Finished {
        command: String,
        outcome: crate::command_line::ShellOutcome,
    },
}

/// Everything the command palette sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandPaletteMessage {
    /// Opens the palette, or gives its field the focus again when it is up.
    Toggle,
    QueryChanged(String),
    Move(isize),
    /// A click on the entry at this position of the list: runs it.
    Select(usize),
    /// Enter: runs the entry under the cursor.
    Run,
    Close,
}
