use std::path::PathBuf;

use ncrs_core::PanelSide;
use ncrs_fs::{Destination, PathProblem};

/// What the command line asks of the app, which it cannot do itself. The app
/// runs it right after `update`.
#[derive(Debug, Clone, PartialEq)]
pub enum CommandLineAction {
    /// The path field ends its editing.
    ClosePathField,
    /// Show this in the status bar as an error.
    ShowError(String),
    /// A `cd` found its destination: load it in the panel, drop the old status
    /// messages, and give the focus back.
    OpenDirectory {
        side: PanelSide,
        destination: Destination,
    },
    /// A `cd` found a problem with `target`: report it.
    ShowPathProblem {
        problem: PathProblem,
        target: PathBuf,
    },
    /// A shell command ended: both panels are read again, and the text is
    /// shown as a notice (`Ok`) or an error (`Err`).
    ShellFinished { report: Result<String, String> },
}
