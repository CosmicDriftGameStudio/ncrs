//! The folders of the index as the Index page edits them: pure functions from
//! the list in the file to the list to write. Sources and targets of filing
//! are not edited here.

use std::path::{Path, PathBuf};

use ncrs_config::config::{FilingLocation, LocationTransfer};
use ncrs_config::space::{SpaceSettings, DEFAULT_WARN_FROM_PERCENT, WARN_FROM_RANGE};
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
    /// Platz: a folder of the user's own that a tool regenerates.
    SpaceRegenerable,
    /// Platz: a folder that is measured but never offered for deleting.
    SpaceNeverTouch,
    /// Platz, App Store build: the home folder, picked once to give access to everything below it.
    SpaceHome,
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

/// `path` joins the Platz list `kind` names; one that is listed already changes
/// nothing, and so does a kind that is no Platz list.
pub fn space_with_folder(current: &SpaceSettings, kind: AddAs, path: PathBuf) -> SpaceSettings {
    let mut next = current.clone();
    let list = match kind {
        AddAs::SpaceRegenerable => &mut next.own_regenerable,
        AddAs::SpaceNeverTouch => &mut next.never_touch,
        AddAs::Indexed
        | AddAs::Favourite
        | AddAs::RepoSource
        | AddAs::RepoSkip
        | AddAs::SpaceHome => return next,
    };
    if !list.contains(&path) {
        list.push(path);
    }
    next
}

/// The warning threshold moved by `step` percent, kept in the range the
/// config takes.
pub fn warn_from_stepped(current: u8, step: i8) -> u8 {
    let moved = i16::from(current) + i16::from(step);
    let (low, high) = (
        i16::from(*WARN_FROM_RANGE.start()),
        i16::from(*WARN_FROM_RANGE.end()),
    );
    u8::try_from(moved.clamp(low, high)).unwrap_or(DEFAULT_WARN_FROM_PERCENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_platz_folder_joins_its_own_list_once() {
        let first = space_with_folder(
            &SpaceSettings::default(),
            AddAs::SpaceRegenerable,
            PathBuf::from("/a"),
        );
        let twice = space_with_folder(&first, AddAs::SpaceRegenerable, PathBuf::from("/a"));
        let both = space_with_folder(&twice, AddAs::SpaceNeverTouch, PathBuf::from("/a"));
        assert_eq!(both.own_regenerable, vec![PathBuf::from("/a")]);
        assert_eq!(both.never_touch, vec![PathBuf::from("/a")]);
    }

    #[test]
    fn the_threshold_stays_between_fifty_and_ninety_nine() {
        assert_eq!(warn_from_stepped(90, 1), 91);
        assert_eq!(warn_from_stepped(99, 1), 99);
        assert_eq!(warn_from_stepped(50, -1), 50);
        assert_eq!(warn_from_stepped(52, -5), 50);
        assert_eq!(warn_from_stepped(97, 5), 99);
    }

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
