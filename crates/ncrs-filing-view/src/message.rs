//! What happens in the filing tab.

use std::collections::BTreeSet;
use std::path::PathBuf;

use ncrs_index::{BatchId, Transfer};

/// What a finished run did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FiledReport {
    pub transfer: Transfer,
    pub done: usize,
    pub trashed: usize,
    /// The files, relative to the source, that stay there.
    pub left: Vec<PathBuf>,
    pub batch: BatchId,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// Take the plan the reading thread has published.
    Tick,
    /// The planning of the session with this id has returned.
    PlanFinished(u64, Result<(), String>),
    /// A file, by its path relative to the source.
    Select(PathBuf),
    SelectNext,
    SelectPrevious,
    StartRename,
    RenameInput(String),
    RenameSubmit,
    RenameCancel,
    PickFolder,
    FolderPicked(Option<PathBuf>),
    /// The file stays where it is.
    Stay,
    ToggleOnlyNew,
    /// The folders of the write folder, listed for the session with this id.
    FoldersListed(u64, BTreeSet<PathBuf>),
    /// The file names read in folders of the write folder, for the session with
    /// this id.
    ExistingRead(u64, Vec<(PathBuf, Vec<String>)>),
    ToggleTrashDupes,
    ToggleFolder(PathBuf),
    File(Transfer),
    Filed(Result<FiledReport, String>),
    Cancel,
    /// The tags of a cancelled plan are written; nothing to do.
    Marked(Result<(), String>),
}
