//! The folders that get a symbol of their own in a panel.

use std::path::{Path, PathBuf};

use ncrs_config::config::FilingLocation;
use ncrs_intake::LocationMode;
use ncrs_ui_kit::file_icon::SpecialFolder;

/// Home, desktop and downloads of the user whose home is `home`. The app adds
/// the folders Hermes reads and files into.
pub fn system_folders(home: &Path) -> Vec<(PathBuf, SpecialFolder)> {
    vec![
        (home.to_path_buf(), SpecialFolder::Home),
        (home.join("Desktop"), SpecialFolder::Desktop),
        (home.join("Downloads"), SpecialFolder::Downloads),
    ]
}

/// The folders with a symbol of their own: the ones Hermes reads and files
/// into first, so a home folder that is also one of them shows that role.
pub fn special_folders_of(
    locations: &[FilingLocation],
    home: &Path,
) -> Vec<(PathBuf, SpecialFolder)> {
    let hermes = locations.iter().filter_map(|location| {
        if location.is_target {
            Some((location.path.clone(), SpecialFolder::WriteFolder))
        } else if location.mode == LocationMode::Inbox {
            Some((location.path.clone(), SpecialFolder::ReadFolder))
        } else {
            None
        }
    });
    hermes.chain(system_folders(home)).collect()
}

/// What `folders` say about `path`. The first entry for a path wins, so a
/// Hermes folder that is also the downloads folder is listed before it.
pub fn special_folder_of(
    folders: &[(PathBuf, SpecialFolder)],
    path: &Path,
) -> Option<SpecialFolder> {
    folders
        .iter()
        .find(|(folder, _)| folder == path)
        .map(|(_, special)| *special)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_entry_for_a_path_wins() {
        let mut folders = vec![(
            PathBuf::from("/home/m/Downloads"),
            SpecialFolder::ReadFolder,
        )];
        folders.extend(system_folders(Path::new("/home/m")));
        assert_eq!(
            special_folder_of(&folders, Path::new("/home/m/Downloads")),
            Some(SpecialFolder::ReadFolder)
        );
        assert_eq!(
            special_folder_of(&folders, Path::new("/home/m")),
            Some(SpecialFolder::Home)
        );
        assert_eq!(
            special_folder_of(&folders, Path::new("/home/m/Music")),
            None
        );
    }

    #[test]
    fn the_hermes_folders_come_before_the_system_ones() {
        let location = |name: &str, mode, is_target| FilingLocation {
            path: PathBuf::from("/home/m").join(name),
            mode,
            images: false,
            is_target,
            transfer: Default::default(),
        };
        let locations = [
            location("Downloads", LocationMode::Inbox, false),
            location("Documents", LocationMode::Archive, true),
            location("Old", LocationMode::Archive, false),
        ];
        let folders = special_folders_of(&locations, Path::new("/home/m"));
        assert_eq!(
            special_folder_of(&folders, Path::new("/home/m/Downloads")),
            Some(SpecialFolder::ReadFolder)
        );
        assert_eq!(
            special_folder_of(&folders, Path::new("/home/m/Documents")),
            Some(SpecialFolder::WriteFolder)
        );
        assert_eq!(special_folder_of(&folders, Path::new("/home/m/Old")), None);
        assert_eq!(
            special_folder_of(&folders, Path::new("/home/m")),
            Some(SpecialFolder::Home)
        );
    }
}
