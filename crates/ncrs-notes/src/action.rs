//! What the notes tab asks of the app.

use std::path::PathBuf;

use ncrs_core::NoteLink;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// The tab is to go.
    Close,
    /// An address for the browser or the mail program.
    OpenUrl(String),
    /// Show the file in the active panel.
    Reveal(PathBuf),
    /// Ask the index where these files are now, given what each note knows
    /// of its file, and answer with
    /// `Message::LinksResolved` with the paths asked, in the same order. Without an index there is
    /// no answer and the paths stay.
    ResolveLinks(Vec<NoteLink>),
    /// Something is wrong with the folder or a file; the text is ready to show.
    Error(String),
    /// Something went well that the person should hear of; the text is ready to show.
    Notice(String),
    /// The recovery key the settings asked for, to be shown once.
    RecoveryKey(String),
}
