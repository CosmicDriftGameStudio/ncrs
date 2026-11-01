//! Showing the hits of a search in a panel, and leaving that list again.

use std::path::{Path, PathBuf};

use iced::Task;
use ncrs_core::PanelSide;

use crate::hit_list::{read_hit_files, HitList, HitListMessage};
use crate::state::Panels;

/// What a hit list message asks of the app, which the panels cannot do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitListAsk {
    /// Escape: leave the list of the active panel and go back to the search.
    BackToSearch,
    /// Leave the list of this panel, nothing else.
    Leave(PanelSide),
    ShowOnOtherSide,
}

/// The hits a search hands to a panel.
#[derive(Debug)]
pub struct Hits {
    pub paths: Vec<PathBuf>,
    pub query: String,
    /// The folder the search ran in.
    pub scope: PathBuf,
}

/// The folder a hit lies in and its file name.
pub fn folder_and_name(hit: &Path) -> Option<(PathBuf, String)> {
    Some((
        hit.parent().map(PathBuf::from)?,
        hit.file_name()
            .map(|name| name.to_string_lossy().into_owned())?,
    ))
}

impl Panels {
    /// Lists `hits` in the panel on `target`. The files are read on the blocking
    /// pool; the panel shows itself loading meanwhile.
    pub fn list_hits(&mut self, target: PanelSide, hits: Hits) -> Task<HitListMessage> {
        let panel = self.panel_mut(target);
        let return_to = panel
            .hit_list
            .as_ref()
            .map_or_else(|| panel.path.clone(), |list| list.return_to.clone());
        let request_id = panel.begin_hit_list(HitList {
            query: hits.query,
            scope: hits.scope,
            return_to,
        });
        self.active = target;
        read_hit_files(target, request_id, hits.paths)
    }

    /// Reads the files of the list again: the rows that vanished drop out.
    pub fn refresh_hit_list(&mut self, side: PanelSide) -> Task<HitListMessage> {
        let panel = self.panel_mut(side);
        let paths: Vec<PathBuf> = panel
            .entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect();
        let request_id = panel.begin_load();
        read_hit_files(side, request_id, paths)
    }

    pub fn update_hit_list(
        &mut self,
        message: HitListMessage,
        visible_rows: usize,
    ) -> Option<HitListAsk> {
        match message {
            HitListMessage::Read {
                side,
                request_id,
                entries,
            } => {
                let panel = self.panel_mut(side);
                if panel.request_id == request_id && panel.is_hit_list() {
                    panel.apply_hit_entries(entries, visible_rows);
                }
                None
            }
            HitListMessage::BackToSearch => Some(HitListAsk::BackToSearch),
            HitListMessage::Close(side) => Some(HitListAsk::Leave(side)),
            HitListMessage::ShowOnOtherSide => Some(HitListAsk::ShowOnOtherSide),
            HitListMessage::RemoveFromList => {
                let panel = self.active_panel_mut();
                if !panel.is_hit_list() {
                    return None;
                }
                let mut paths: Vec<PathBuf> = panel
                    .entries
                    .iter()
                    .filter(|entry| panel.selection.is_tagged(&entry.path))
                    .map(|entry| entry.path.clone())
                    .collect();
                if paths.is_empty() {
                    paths.extend(panel.selected_entry().map(|entry| entry.path.clone()));
                }
                panel.remove_hits(&paths, visible_rows);
                None
            }
        }
    }

    /// Drops the list of `side`; the folder it came from is what to show next.
    pub fn leave_hit_list(&mut self, side: PanelSide) -> Option<PathBuf> {
        let list = self.panel_mut(side).drop_hit_list()?;
        self.context_menu = None;
        Some(list.return_to)
    }

    /// The panel `other` shows a folder next, so its hit list is gone.
    pub fn leave_hit_list_for_hit(&mut self, other: PanelSide) {
        self.panel_mut(other).drop_hit_list();
        self.active = other;
    }
}
