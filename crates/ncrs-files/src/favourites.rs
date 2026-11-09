//! The favourites bar: which folders get a slot, what the slots are called,
//! opening one and moving or removing it.

use std::path::{Path, PathBuf};

use ncrs_core::PanelSide;
use ncrs_i18n::Language;
use ncrs_ui_kit::favourites_bar::FAVOURITES_BAR_SLOTS as BAR_SLOTS;

use crate::action::Action;
use crate::state::Panels;
use crate::volume_flow::favourite_target;
use crate::volumes_view::FavouriteRow;

fn last_component(path: &Path, home: Option<&Path>) -> String {
    if home == Some(path) {
        return "~".to_owned();
    }
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The labels of the first favourites the bar has slots for: the last path
/// component, `~` for home, and `parent/name` where two of them share a name.
pub fn slot_labels(favourites: &[PathBuf], home: Option<&Path>) -> Vec<String> {
    let shown = favourites.iter().take(BAR_SLOTS).collect::<Vec<_>>();
    let names: Vec<String> = shown
        .iter()
        .map(|path| last_component(path, home))
        .collect();
    shown
        .iter()
        .zip(&names)
        .map(|(path, name)| {
            let is_shared = names.iter().filter(|other| *other == name).count() > 1;
            match path.parent().filter(|_| is_shared) {
                Some(parent) if parent.file_name().is_some() => {
                    format!("{}/{name}", last_component(parent, home))
                }
                _ => name.clone(),
            }
        })
        .collect()
}

impl Panels {
    /// The slot of the active panel's folder, when it is one of the bar's.
    pub fn current_slot(&self) -> Option<usize> {
        let here = &self.panel(self.active).path;
        self.favourites
            .iter()
            .take(BAR_SLOTS)
            .position(|favourite| favourite == here)
    }

    /// Opens favourite `index` in `side`, the way Enter on it in the drive menu
    /// does. An index without a slot does nothing.
    pub fn open_favourite(&mut self, index: usize, side: PanelSide, lang: Language) -> Vec<Action> {
        let Some(path) = self.favourites.get(index).filter(|_| index < BAR_SLOTS) else {
            return Vec::new();
        };
        let row = FavouriteRow {
            present: path.is_dir(),
            path: path.clone(),
        };
        match favourite_target(&row, lang) {
            Err(action) => vec![action],
            Ok(target) => {
                self.active = side;
                vec![
                    Action::ClearMessages,
                    Action::Load {
                        side,
                        path: target,
                        select: None,
                    },
                ]
            }
        }
    }

    /// Moves favourite `index` one place up (`-1`) or down (`1`). Returns
    /// whether the order changed.
    pub fn move_favourite(&mut self, index: usize, step: isize) -> bool {
        let Some(target) = index.checked_add_signed(step) else {
            return false;
        };
        if index >= self.favourites.len() || target >= self.favourites.len() {
            return false;
        }
        self.favourites.swap(index, target);
        true
    }

    /// Removes favourite `index`. Returns whether there was one.
    pub fn remove_favourite_index(&mut self, index: usize) -> bool {
        if index >= self.favourites.len() {
            return false;
        }
        self.favourites.remove(index);
        true
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here.
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::PanelState;

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    fn panels(list: &[&str]) -> Panels {
        let mut panels = Panels::new(
            PanelState::new(PathBuf::from("/a")),
            PanelState::new(PathBuf::from("/b")),
            PanelSide::Left,
        );
        panels.favourites = paths(list);
        panels
    }

    #[test]
    fn home_is_a_tilde_and_root_stays_a_slash() {
        let labels = slot_labels(
            &paths(&["/Users/me", "/", "/data/docs"]),
            Some(Path::new("/Users/me")),
        );
        assert_eq!(labels, ["~", "/", "docs"]);
    }

    #[test]
    fn equal_names_get_their_parent() {
        let labels = slot_labels(&paths(&["/a/src", "/b/src", "/c/docs"]), None);
        assert_eq!(labels, ["a/src", "b/src", "docs"]);
    }

    #[test]
    fn only_five_get_a_slot() {
        let labels = slot_labels(&paths(&["/1", "/2", "/3", "/4", "/5", "/6"]), None);
        assert_eq!(labels.len(), 5);
        // A sixth with the name of a fifth does not rename the fifth.
        let with_a_sixth_twin =
            slot_labels(&paths(&["/1", "/2", "/3", "/4", "/x/5", "/y/5"]), None);
        assert_eq!(with_a_sixth_twin[4], "5");
    }

    #[test]
    fn moving_keeps_the_order_of_the_rest() {
        let mut panels = panels(&["/1", "/2", "/3"]);
        assert!(panels.move_favourite(2, -1));
        assert_eq!(panels.favourites, paths(&["/1", "/3", "/2"]));
        assert!(!panels.move_favourite(0, -1));
        assert!(!panels.move_favourite(2, 1));
    }

    #[test]
    fn removing_keeps_the_order_of_the_rest() {
        let mut panels = panels(&["/1", "/2", "/3"]);
        assert!(panels.remove_favourite_index(1));
        assert_eq!(panels.favourites, paths(&["/1", "/3"]));
        assert!(!panels.remove_favourite_index(5));
    }

    #[test]
    fn a_number_without_a_favourite_does_nothing() {
        let mut panels = panels(&["/1"]);
        assert!(panels
            .open_favourite(3, PanelSide::Left, Language::English)
            .is_empty());
    }
}
