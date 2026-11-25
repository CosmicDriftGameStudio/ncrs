//! The folders that get a symbol of their own in a panel.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use ncrs_ui_kit::file_icon::SpecialFolder;

use crate::platform::Platform;

/// Home, desktop and downloads of the user whose home is `home`. The app adds
/// the folders Hermes reads and files into.
pub fn system_folders(home: &Path) -> Vec<(PathBuf, SpecialFolder)> {
    let mut folders = vec![
        (home.to_path_buf(), SpecialFolder::Home),
        (home.join("Desktop"), SpecialFolder::Desktop),
        (home.join("Downloads"), SpecialFolder::Downloads),
    ];
    folders.extend(programs_folder().map(|folder| (folder, SpecialFolder::Programs)));
    folders
}

/// The folder of the installed programs on this system; none on Linux, where
/// the launchers are spread over several folders.
pub fn programs_folder() -> Option<PathBuf> {
    programs_folder_of(Platform::current(), std::env::var_os("ProgramFiles"))
}

/// [`programs_folder`] for `platform`, with `program_files` the value of the
/// `ProgramFiles` variable.
pub fn programs_folder_of(platform: Platform, program_files: Option<OsString>) -> Option<PathBuf> {
    match platform {
        Platform::Mac => Some(PathBuf::from("/Applications")),
        Platform::Windows => Some(
            program_files
                .filter(|folder| !folder.is_empty())
                .map_or_else(|| PathBuf::from(r"C:\Program Files"), PathBuf::from),
        ),
        Platform::Unix => None,
    }
}

/// The favourites of a first start, when no state of an earlier run exists:
/// the programs folder, where there is one on this system.
pub fn first_start_favourites() -> Vec<PathBuf> {
    programs_folder()
        .filter(|folder| folder.is_dir())
        .into_iter()
        .collect()
}

/// The folders with a symbol of their own: the targets and the sources of
/// Hermes first, so a home folder that is also one of them shows that role,
/// and one that is both shows the target.
pub fn special_folders_of<'a>(
    targets: impl IntoIterator<Item = &'a Path>,
    sources: impl IntoIterator<Item = &'a Path>,
    home: &Path,
) -> Vec<(PathBuf, SpecialFolder)> {
    let targets = targets
        .into_iter()
        .map(|path| (path.to_path_buf(), SpecialFolder::WriteFolder));
    let sources = sources
        .into_iter()
        .map(|path| (path.to_path_buf(), SpecialFolder::ReadFolder));
    targets.chain(sources).chain(system_folders(home)).collect()
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
    fn the_programs_folder_is_per_system() {
        assert_eq!(
            programs_folder_of(Platform::Mac, None),
            Some(PathBuf::from("/Applications"))
        );
        assert_eq!(
            programs_folder_of(Platform::Windows, Some(r"D:\Programs".into())),
            Some(PathBuf::from(r"D:\Programs"))
        );
        assert_eq!(
            programs_folder_of(Platform::Windows, None),
            Some(PathBuf::from(r"C:\Program Files"))
        );
        assert_eq!(programs_folder_of(Platform::Unix, None), None);
    }

    #[test]
    fn the_hermes_folders_come_before_the_system_ones_and_a_target_before_a_source() {
        let home = Path::new("/home/m");
        let [downloads, documents, both] =
            ["Downloads", "Documents", "Both"].map(|name| home.join(name));
        let folders = special_folders_of(
            [documents.as_path(), both.as_path()],
            [downloads.as_path(), both.as_path()],
            home,
        );
        assert_eq!(
            special_folder_of(&folders, &downloads),
            Some(SpecialFolder::ReadFolder)
        );
        assert_eq!(
            special_folder_of(&folders, &documents),
            Some(SpecialFolder::WriteFolder)
        );
        assert_eq!(
            special_folder_of(&folders, &both),
            Some(SpecialFolder::WriteFolder)
        );
        assert_eq!(special_folder_of(&folders, &home.join("Old")), None);
        assert_eq!(special_folder_of(&folders, home), Some(SpecialFolder::Home));
    }
}
