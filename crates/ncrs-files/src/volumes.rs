//! The open drive menu: the places the panel can change to, and the highlight.

use std::path::Path;

use ncrs_core::PanelSide;
use ncrs_fs::Volume;
use ncrs_i18n::Msg;

use crate::volumes_view::{FavouriteRow, ServerRow};

/// The open drive menu. The list is read when the menu opens, so a drive
/// plugged in a moment ago is there and one removed since is gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeMenu {
    /// The panel that changes drive.
    pub side: PanelSide,
    pub volumes: Vec<Volume>,
    /// Listed below the drives; `present` is read when the menu opens.
    pub favourites: Vec<FavouriteRow>,
    /// The servers, below the favourites.
    pub servers: Vec<ServerRow>,
    /// The last row, below the servers: what the trash is called here, or
    /// nothing in a build that has no trash to show.
    pub trash: Option<Msg>,
    /// The highlighted row, the one Enter goes to. Counts the drives first,
    /// then the favourites, then the servers, then the trash.
    pub selected: usize,
}

impl VolumeMenu {
    /// Opens on the drive `current` lives on: the volume with the longest path
    /// that `current` starts with.
    pub fn new(side: PanelSide, volumes: Vec<Volume>, current: &Path) -> Self {
        let selected = volumes
            .iter()
            .enumerate()
            .filter(|(_, volume)| current.starts_with(&volume.path))
            .max_by_key(|(index, volume)| (volume.path.components().count(), usize::MAX - index))
            .map_or(0, |(index, _)| index);
        Self {
            side,
            volumes,
            favourites: Vec::new(),
            servers: Vec::new(),
            trash: None,
            selected,
        }
    }

    pub fn with_trash(mut self, trash: Option<Msg>) -> Self {
        self.trash = trash;
        self
    }

    pub fn with_favourites(mut self, favourites: Vec<FavouriteRow>) -> Self {
        self.favourites = favourites;
        self
    }

    pub fn with_servers(mut self, servers: Vec<ServerRow>) -> Self {
        self.servers = servers;
        self
    }

    /// Drives, favourites and servers together: what `selected` runs over.
    pub fn row_count(&self) -> usize {
        self.volumes.len()
            + self.favourites.len()
            + self.servers.len()
            + usize::from(self.trash.is_some())
    }

    pub fn last_row(&self) -> usize {
        self.row_count().saturating_sub(1)
    }

    pub fn move_by(&mut self, delta: isize) {
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.last_row());
    }

    /// The index among the favourites when the highlight is on one.
    pub fn selected_favourite(&self) -> Option<usize> {
        self.selected
            .checked_sub(self.volumes.len())
            .filter(|index| *index < self.favourites.len())
    }

    /// Whether the highlight is on the trash row.
    pub fn selected_trash(&self) -> bool {
        self.trash.is_some() && self.selected + 1 == self.row_count()
    }

    /// The index among the servers when the highlight is on one.
    pub fn selected_server(&self) -> Option<usize> {
        self.selected
            .checked_sub(self.volumes.len() + self.favourites.len())
            .filter(|index| *index < self.servers.len())
    }
}
