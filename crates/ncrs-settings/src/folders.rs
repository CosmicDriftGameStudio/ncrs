//! The list of read folders as the Index and Hermes pages edit it: pure
//! functions from the list in the file to the list to write.

use std::path::{Path, PathBuf};

use ncrs_config::config::{FilingLocation, LocationTransfer};
use ncrs_intake::LocationMode;

/// Which page a folder is added from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddAs {
    /// Hermes: a folder new documents arrive in.
    ReadFolder,
    /// Index: a folder that is only read where it is.
    Indexed,
    /// Favourites: a folder for the bar under the tabs and the drive menu.
    Favourite,
}

/// `path` joins the list. An indexed folder becomes the filing target when
/// there is none yet; a folder that is listed already changes nothing.
pub fn added(locations: &[FilingLocation], path: PathBuf, kind: AddAs) -> Vec<FilingLocation> {
    let mut list = locations.to_vec();
    if list.iter().any(|known| known.path == path) {
        return list;
    }
    let is_target = kind == AddAs::Indexed && !list.iter().any(|known| known.is_target);
    let mode = match kind {
        AddAs::ReadFolder => LocationMode::Inbox,
        AddAs::Indexed => LocationMode::Archive,
        // A favourite is not a location of the filing.
        AddAs::Favourite => return list,
    };
    list.push(FilingLocation {
        path,
        mode,
        images: false,
        is_target,
        transfer: LocationTransfer::default(),
    });
    list
}

/// `path` leaves the list. When it was the target, the first indexed folder
/// left takes over; with none left there is no target.
pub fn removed(locations: &[FilingLocation], path: &Path) -> Vec<FilingLocation> {
    let was_target = locations
        .iter()
        .any(|known| known.path == path && known.is_target);
    let mut list: Vec<FilingLocation> = locations
        .iter()
        .filter(|known| known.path != path)
        .cloned()
        .collect();
    if was_target {
        if let Some(heir) = list
            .iter_mut()
            .find(|known| known.mode == LocationMode::Archive)
        {
            heir.is_target = true;
        }
    }
    list
}

/// The location with the path of `edited` takes its values.
pub fn replaced(locations: &[FilingLocation], edited: FilingLocation) -> Vec<FilingLocation> {
    locations
        .iter()
        .map(|known| {
            if known.path == edited.path {
                edited.clone()
            } else {
                known.clone()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn location(path: &str, mode: LocationMode, is_target: bool) -> FilingLocation {
        FilingLocation {
            path: PathBuf::from(path),
            mode,
            images: false,
            is_target,
            transfer: LocationTransfer::Move,
        }
    }

    #[test]
    fn a_read_folder_is_added_as_inbox_and_never_as_target() {
        let list = added(&[], PathBuf::from("/in"), AddAs::ReadFolder);
        assert_eq!(list, vec![location("/in", LocationMode::Inbox, false)]);
    }

    #[test]
    fn the_first_indexed_folder_becomes_the_target_and_the_second_does_not() {
        let first = added(&[], PathBuf::from("/a"), AddAs::Indexed);
        let both = added(&first, PathBuf::from("/b"), AddAs::Indexed);
        assert_eq!(
            both,
            vec![
                location("/a", LocationMode::Archive, true),
                location("/b", LocationMode::Archive, false),
            ]
        );
    }

    #[test]
    fn a_folder_already_listed_is_not_added_again() {
        let list = vec![location("/a", LocationMode::Archive, true)];
        assert_eq!(added(&list, PathBuf::from("/a"), AddAs::ReadFolder), list);
    }

    #[test]
    fn removing_the_target_hands_it_to_the_first_indexed_folder() {
        let list = vec![
            location("/in", LocationMode::Inbox, false),
            location("/a", LocationMode::Archive, true),
            location("/b", LocationMode::Archive, false),
        ];
        assert_eq!(
            removed(&list, Path::new("/a")),
            vec![
                location("/in", LocationMode::Inbox, false),
                location("/b", LocationMode::Archive, true),
            ]
        );
    }

    #[test]
    fn removing_the_last_indexed_folder_leaves_no_target() {
        let list = vec![
            location("/in", LocationMode::Inbox, false),
            location("/a", LocationMode::Archive, true),
        ];
        assert_eq!(
            removed(&list, Path::new("/a")),
            vec![location("/in", LocationMode::Inbox, false)]
        );
    }

    #[test]
    fn removing_a_read_folder_keeps_the_target() {
        let list = vec![
            location("/in", LocationMode::Inbox, false),
            location("/a", LocationMode::Archive, true),
        ];
        assert_eq!(
            removed(&list, Path::new("/in")),
            vec![location("/a", LocationMode::Archive, true)]
        );
    }

    #[test]
    fn an_edited_folder_replaces_only_its_own_entry() {
        let list = vec![
            location("/in", LocationMode::Inbox, false),
            location("/other", LocationMode::Inbox, false),
        ];
        let edited = FilingLocation {
            images: true,
            transfer: LocationTransfer::Copy,
            ..location("/in", LocationMode::Archive, false)
        };
        assert_eq!(
            replaced(&list, edited.clone()),
            vec![edited, location("/other", LocationMode::Inbox, false)]
        );
    }
}
