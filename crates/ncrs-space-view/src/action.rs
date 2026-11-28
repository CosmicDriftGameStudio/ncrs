//! What the Platz tab asks of the app.

use std::path::PathBuf;
use std::sync::Arc;

use ncrs_filing::execute::OwnTrashItem;
use ncrs_fs::transfer::Tick;
use ncrs_space::{RunLog, Scan};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Show this folder in the active file panel.
    ShowInPanel(PathBuf),
    /// Open the Hermes chat on the space card.
    AskHermes,
    /// Ask the user for access to the home folder (the sandbox build); the tab measures again once it is given.
    GrantHome,
    /// A measure finished: Hermes and the Monitor read the same numbers.
    Measured(Arc<Scan>),
    /// A delete run finished.
    RunDone(Arc<RunLog>),
    /// The run that deletes has come this far.
    SetJobProgress(Tick),
    ClearJobProgress,
    /// Put these files of the user's own in the trash, as a Hermes run with a journal, so that undo brings them back.
    /// `root` is the folder they lie in. The answer comes back as `DecideMessage::Trashed`.
    TrashOwn {
        root: PathBuf,
        items: Vec<OwnTrashItem>,
        empty_after: bool,
    },
    /// Undo the last trash run of the tab; the answer comes back as `DecideMessage::Undone`.
    UndoTrash,
    /// Nothing is left to leave with Esc: the app may close the tab.
    Closed,
}
