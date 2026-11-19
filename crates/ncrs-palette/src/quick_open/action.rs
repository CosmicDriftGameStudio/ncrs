use std::path::PathBuf;

use super::DocumentRequest;

/// What Quick Open asks of the app, which it cannot do itself. The app runs it
/// right after `update`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuickOpenAction {
    /// Close the box and stop its walk.
    Close,
    /// A `>` was typed first: close the box and open the command list with
    /// this query.
    SwitchToCommands(String),
    /// Look up the documents of this text on a blocking thread.
    FindDocuments(DocumentRequest),
    /// Enter on the last row: close the box and open the search with this text.
    OpenSearch(String),
    /// Enter on a hit: close the box and show the file in the active panel.
    Reveal(PathBuf),
}
