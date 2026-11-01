//! The list of search hits a panel can show in place of a folder.

use std::path::PathBuf;

use ncrs_core::{FileEntry, PanelSide};

/// A flat list of search hits that stands in for the folder listing. The
/// entries are a snapshot; `scope` is the folder the search ran in and the
/// path the panel reports, `return_to` the folder the panel showed before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HitList {
    pub query: String,
    pub scope: PathBuf,
    pub return_to: PathBuf,
}

/// Everything a panel that lists search hits sends.
#[derive(Debug, Clone, PartialEq)]
pub enum HitListMessage {
    /// The snapshot of the hits was read (or read again). Dropped when
    /// `request_id` is not the panel's latest.
    Read {
        side: PanelSide,
        request_id: u64,
        entries: Vec<FileEntry>,
    },
    /// Escape: leave the list and go back to the search.
    BackToSearch,
    /// The cross of the tab: leave the list, nothing else.
    Close(PanelSide),
    /// Context menu: show the file under the cursor in its folder on the other
    /// side.
    ShowOnOtherSide,
    /// Context menu: take the tagged rows, else the cursor row, off the list.
    RemoveFromList,
}
