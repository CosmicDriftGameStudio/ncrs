//! The folders of the index as the Index page edits them: pure functions from
//! the list in the file to the list to write. Sources and targets of filing
//! are not edited here.

use std::path::{Path, PathBuf};

use ncrs_config::config::{FilingLocation, LocationTransfer};
use ncrs_intake::LocationMode;

/// Which page a folder is added from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddAs {
    /// Index: a folder that is only read where it is.
    Indexed,
    /// Favourites: a folder for the bar under the tabs and the drive menu.
    Favourite,
    /// Git: a folder the Repos tab searches for repositories.
    RepoSource,
    /// Git: a folder name the search does not enter.
    RepoSkip,
}

/// `path` joins the list as a folder that is only read; a folder that is
/// listed already changes nothing.
pub fn added(locations: &[FilingLocation], path: PathBuf) -> Vec<FilingLocation> {
    let mut list = locations.to_vec();
    if list.iter().any(|known| known.path == path) {
        return list;
    }
    list.push(FilingLocation {
        path,
        mode: LocationMode::Archive,
        images: false,
        is_target: false,
        transfer: LocationTransfer::default(),
        continuous: false,
    });
    list
}

/// `path` leaves the list.
pub fn removed(locations: &[FilingLocation], path: &Path) -> Vec<FilingLocation> {
    locations
        .iter()
        .filter(|known| known.path != path)
        .cloned()
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
            continuous: false,
        }
    }

    #[test]
    fn an_added_folder_is_only_read_and_never_a_target() {
        let first = added(&[], PathBuf::from("/a"));
        let both = added(&first, PathBuf::from("/b"));
        assert_eq!(
            both,
            vec![
                location("/a", LocationMode::Archive, false),
                location("/b", LocationMode::Archive, false),
            ]
        );
    }

    #[test]
    fn a_folder_already_listed_is_not_added_again() {
        let list = vec![location("/a", LocationMode::Archive, true)];
        assert_eq!(added(&list, PathBuf::from("/a")), list);
    }

    #[test]
    fn removing_a_folder_leaves_sources_and_targets_as_they_are() {
        let list = vec![
            location("/in", LocationMode::Inbox, false),
            location("/a", LocationMode::Archive, true),
            location("/b", LocationMode::Archive, false),
        ];
        assert_eq!(
            removed(&list, Path::new("/b")),
            vec![
                location("/in", LocationMode::Inbox, false),
                location("/a", LocationMode::Archive, true),
            ]
        );
    }
}
