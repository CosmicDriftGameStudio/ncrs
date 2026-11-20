//! The favourites bar: which folders get a slot, what the slots are called,
//! opening one and moving or removing it.

use std::path::{Path, PathBuf};

use ncrs_core::PanelSide;
use ncrs_i18n::Language;

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

/// The labels of the favourites: the last path component, `~` for home, and
/// `parent/name` where two of them share a name.
pub fn slot_labels(favourites: &[PathBuf], home: Option<&Path>) -> Vec<String> {
    let shown: Vec<&PathBuf> = favourites.iter().collect();
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
    /// The place in the list of the active panel's folder, when it is a favourite.
    pub fn current_slot(&self) -> Option<usize> {
        let here = &self.panel(self.active).path;
        self.favourites
            .iter()
            .position(|favourite| favourite == here)
    }

    /// Opens favourite `index` in `side`, the way Enter on it in the drive menu
    /// does. An index without a favourite does nothing.
    pub fn open_favourite(&mut self, index: usize, side: PanelSide, lang: Language) -> Vec<Action> {
        let Some(path) = self.favourites.get(index) else {
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
    fn every_favourite_gets_a_label_without_a_limit() {
        let many: Vec<String> = (0..30).map(|number| format!("/{number}")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        assert_eq!(slot_labels(&paths(&many), None).len(), 30);
        // A twin far down the list still gets its parent.
        let twins = slot_labels(
            &paths(&["/1", "/2", "/3", "/4", "/5", "/6", "/x/7", "/y/7"]),
            None,
        );
        assert_eq!(twins[6], "x/7");
    }

    #[test]
    fn a_favourite_past_the_ninth_still_opens() {
        let mut panels = panels(&["/1", "/2", "/3", "/4", "/5", "/6", "/7", "/8", "/9", "/10"]);
        assert!(!panels
            .open_favourite(9, PanelSide::Left, Language::English)
            .is_empty());
        assert_eq!(panels.current_slot(), None);
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
