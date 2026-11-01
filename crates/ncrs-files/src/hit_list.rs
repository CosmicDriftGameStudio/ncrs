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

/// The archive name a hit list suggests: its query, when that is one plain
/// file name on every platform.
pub fn hit_archive_name(typed: &str) -> Option<String> {
    let query = typed.trim();
    let forbidden = |c: char| c.is_control() || "<>:\"/\\|?*".contains(c);
    let plain = !query.chars().any(forbidden)
        && matches!(
            ncrs_fs::archive::safe_components(query).as_deref(),
            Some([only]) if only == query
        );
    plain.then(|| format!("{query}.zip"))
}

/// Reads the files of a hit list off the UI thread. A file that is gone is
/// left out.
pub fn read_hit_files(
    side: PanelSide,
    request_id: u64,
    paths: Vec<PathBuf>,
) -> iced::Task<HitListMessage> {
    iced::Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                paths
                    .iter()
                    .filter_map(|path| FileEntry::from_path(path))
                    .collect::<Vec<_>>()
            })
            .await
            .unwrap_or_default()
        },
        move |entries| HitListMessage::Read {
            side,
            request_id,
            entries,
        },
    )
}
