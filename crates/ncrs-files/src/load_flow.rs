//! Starting the read of a folder into a panel, and the reload that waits for
//! an operation.

use std::path::PathBuf;

use iced::Task;
use ncrs_core::PanelSide;
use ncrs_fs::remote::RemotePath;

use crate::message::PanelMessage;
use crate::state::Panels;

/// What `Panels::start_load` leaves to the app.
#[derive(Debug)]
pub enum LoadStart {
    /// The panel shows search hits and is asked for its own folder: the hits
    /// are read again.
    RefreshHitList,
    /// The path is on a server: the session is the app's.
    Remote(RemotePath),
    /// The read of a folder of this computer.
    Local(Task<PanelMessage>),
}

impl Panels {
    /// Starts the read of `path` into `side`.
    pub fn start_load(
        &mut self,
        side: PanelSide,
        path: PathBuf,
        select: Option<String>,
    ) -> LoadStart {
        if self.panel(side).is_hit_list() {
            if path == self.panel(side).path {
                return LoadStart::RefreshHitList;
            }
            self.panel_mut(side).drop_hit_list();
        }
        // A server is told from the text of the path before anything else
        // looks at it: nothing here may hand such a path to the local disk.
        if let Some(remote) = RemotePath::parse(&path) {
            return LoadStart::Remote(remote);
        }
        let panel = self.panel_mut(side);
        let request_id = panel.begin_load();
        LoadStart::Local(Task::perform(
            ncrs_fs::read_directory(path, panel.sort),
            move |listing| PanelMessage::DirectoryLoaded {
                side,
                request_id,
                path: listing.path,
                entries: listing.entries,
                error: listing.error,
                sorted_by: listing.sorted_by,
                select: select.clone(),
                archive: listing.archive,
            },
        ))
    }
}
