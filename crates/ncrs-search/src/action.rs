use ncrs_core::PanelSide;
use ncrs_index::{QuerySpec, SavedQueryId};

/// What the search asks the app for, because it reaches outside the search.
/// The app runs it right after `update`.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Close,
    OpenHitList(PanelSide),
    /// Shows the file under the cursor in the other panel.
    ActivateCursor,
    OpenContextMenu,
    /// The Reports page after the search handed its criteria over: with the
    /// criteria of a new report, or for the report the search changed.
    HandToReports {
        unsaved: Option<QuerySpec>,
        report: Option<SavedQueryId>,
    },
}
