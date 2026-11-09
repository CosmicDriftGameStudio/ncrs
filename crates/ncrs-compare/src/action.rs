use std::path::PathBuf;

use ncrs_core::PanelSide;

/// What the comparison asks of the app, which it cannot do itself. The app runs
/// the actions in order, right after `update`.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    CloseContextMenu,
    ShowError(String),
    /// The marks or the filter changed: the panels take them again.
    MarksChanged,
    /// Copies `sources` into the folder `target` through the copy queue, which
    /// asks about files that exist.
    Copy {
        sources: Vec<PathBuf>,
        target: PathBuf,
    },
    /// Asks, in the delete dialog, to delete `sources` of the panel on `side`.
    Delete {
        side: PanelSide,
        sources: Vec<PathBuf>,
    },
}
