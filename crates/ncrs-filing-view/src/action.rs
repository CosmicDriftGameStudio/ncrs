//! What the filing tab asks of the app.

use std::path::PathBuf;

use ncrs_index::{BatchId, DocumentId, TargetId};
use ncrs_intake::LocationTransfer;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// The tab is to go.
    Close,
    /// Ask the user for a folder, starting in `start`, and answer with
    /// `Message::FolderPicked`.
    PickFolder { start: PathBuf },
    /// Open the rule form for the file, and send `Message::RulesChanged` once a
    /// rule was saved.
    OpenRuleForm {
        /// The target the file has to do with; the first of the source where
        /// no rule takes it.
        target: Option<TargetId>,
        document: Option<DocumentId>,
        path: PathBuf,
    },
    /// Ask Hermes for a rule that fits the file. `source` is the folder of the
    /// source.
    SuggestRule { source: PathBuf, path: PathBuf },
    /// Files were filed or trashed; the tab stays.
    Filed {
        source: PathBuf,
        transfer: LocationTransfer,
        filed: usize,
        /// How many files still lie in the source without a place.
        left: usize,
        /// For the undo; `None` when nothing was begun.
        batch: Option<BatchId>,
    },
}
