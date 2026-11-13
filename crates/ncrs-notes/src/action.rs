//! What the notes tab asks of the app.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// The tab is to go.
    Close,
    /// An address for the browser or the mail program.
    OpenUrl(String),
    /// Show the file in the active panel.
    Reveal(PathBuf),
    /// Ask the index for `(document id, current path)` of these files, as
    /// `(known document id, path in the note)`, and answer with
    /// `Message::LinksResolved` in the same order. Without an index there is
    /// no answer and the paths stay.
    ResolveLinks(Vec<(Option<i64>, PathBuf)>),
    /// Something is wrong with the folder or a file; the text is ready to show.
    Error(String),
}
