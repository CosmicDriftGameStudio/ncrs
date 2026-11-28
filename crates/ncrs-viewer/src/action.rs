//! What the viewer asks of the app.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// The disk warning in the status bar was pressed: ask Hermes why the drive is full.
    AskWhyDiskFull,
    /// Esc: back to the panels.
    Close,
    /// An address for the browser or the mail program.
    OpenUrl(String),
    /// Show the file in the active panel.
    Reveal(PathBuf),
    /// Open the file with the program the system has for it.
    OpenExternal(PathBuf),
    /// Open the file in the editor (F4).
    Edit(PathBuf),
    /// The file is too big for the tab: open it with the system and say so.
    OpenSystemInstead {
        path: PathBuf,
        notice: String,
    },
    Error(String),
}
