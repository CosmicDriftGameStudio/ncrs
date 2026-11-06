//! One subscription per panel that shows a folder of this computer.

use std::path::PathBuf;

use iced::Subscription;
use ncrs_core::PanelSide;

use crate::message::PanelMessage;
use crate::state::Panels;

impl Panels {
    /// The watches of the panels. A subscription is identified by side and
    /// folder, so it restarts exactly when the panel goes to another folder.
    /// Archives, servers and hit lists send no events; they keep the manual
    /// reload.
    pub fn folder_watches(&self) -> Subscription<PanelMessage> {
        Subscription::batch(
            [PanelSide::Left, PanelSide::Right]
                .into_iter()
                .filter(|side| {
                    let panel = self.panel(*side);
                    !panel.is_virtual() && !panel.is_hit_list()
                })
                .map(|side| {
                    Subscription::run_with((side, self.panel(side).path.clone()), watch_folder)
                }),
        )
    }
}

fn watch_folder(
    (side, folder): &(PanelSide, PathBuf),
) -> impl iced::futures::Stream<Item = PanelMessage> + 'static {
    let side = *side;
    iced::futures::StreamExt::map(ncrs_fs::folder_watch::folder_changes(folder), move |()| {
        PanelMessage::FolderChangedOnDisk(side)
    })
}
