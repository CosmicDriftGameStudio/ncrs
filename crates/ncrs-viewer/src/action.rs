//! What the viewer asks of the app.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
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
