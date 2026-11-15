//! What the filing tab asks of the app.

use std::path::PathBuf;

use ncrs_index::{BatchId, Transfer};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// The tab is to go.
    Close,
    /// Ask the user for a folder, starting in `start`, and answer with
    /// `Message::FolderPicked`.
    PickFolder { start: PathBuf },
    /// The run is done and the tab is to go. `left` are the files, relative to
    /// `source`, that stay there.
    Finished {
        source: PathBuf,
        transfer: Transfer,
        filed: usize,
        left: Vec<PathBuf>,
        batch: BatchId,
    },
}
