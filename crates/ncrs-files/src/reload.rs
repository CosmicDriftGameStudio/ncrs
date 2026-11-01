//! Reading the panels again: the reload that waits for an operation, and the
//! questions about what the panels show.

use std::path::{Path, PathBuf};

use ncrs_core::PanelSide;

use crate::state::Panels;

/// A reload waiting for its operation to finish.
///
/// F7 creates a directory and then reads the panel. Both were started at once,
/// which races: a read that finishes first does not see the new entry. The
/// reload waits here until the create has reported back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingReload {
    pub side: PanelSide,
    /// Name to select afterwards, so Enter opens what was just created.
    pub name: String,
}

impl Panels {
    /// Whether a panel is in `place`: an archive, or the root address of the
    /// server whose folder it shows.
    pub fn shows(&self, place: &Path) -> bool {
        [PanelSide::Left, PanelSide::Right].into_iter().any(|side| {
            let panel = self.panel(side);
            panel.archive.as_deref() == Some(place)
                || panel
                    .remote()
                    .is_some_and(|server| server.root_path() == place)
        })
    }

    /// The panels that are folders of this computer, with their paths: the
    /// ones in an archive or on a server are left out.
    pub fn local_folders(&self) -> Vec<(PanelSide, PathBuf)> {
        [PanelSide::Left, PanelSide::Right]
            .into_iter()
            .filter_map(|side| {
                let panel = self.panel(side);
                (!panel.is_virtual()).then(|| (side, panel.path.clone()))
            })
            .collect()
    }

    /// The rows the menu was opened on are gone. It acts on the active panel,
    /// so only that panel's reload closes it.
    pub fn close_menu_of_reloaded(&mut self, side: PanelSide) {
        if side == self.active {
            self.context_menu = None;
        }
    }
}
