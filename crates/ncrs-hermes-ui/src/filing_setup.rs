//! The list of folders Hermes works on: the rules it has to follow, the role
//! a folder can take, and writing the list to `[modules.filing]` in the config
//! file. Looking up a folder and writing the file touch the disk and run on a
//! blocking thread; the app hands their results back.

use std::path::{Path, PathBuf};

use ncrs_intake::LocationMode;

use crate::hermes_column::FolderRole;
use ncrs_config::config::{self, Filing, FilingLocation, FilingRuleViolation, FilingWriteError};
pub use ncrs_ui_kit::format::shown_path;

/// What the card says about the last thing the user did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupNote {
    FolderUnusable {
        folder: PathBuf,
        reason: String,
    },
    /// Not a failure: the folder is in the list, nothing was added.
    AlreadyAdded {
        folder: PathBuf,
    },
    NoConfigPath,
    SaveFailed(FilingWriteError),
    /// The folder is chosen, but the access to it could not be kept for the
    /// next start.
    GrantFailed {
        folder: PathBuf,
        reason: String,
    },
}

/// What keeps the list from being saved. Shown at once, so the user sees what
/// to fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupProblem {
    NoDocumentFolder,
    ChooseFilingLocation,
    Rule(FilingRuleViolation),
}

#[derive(Debug, Default)]
pub struct FilingSetup {
    /// In the order the folders were added. Every index the card sends points
    /// into this list, whichever section shows the entry.
    chosen: Vec<FilingLocation>,
}

impl FilingSetup {
    /// Starts from the locations the config lists now, so saving after one
    /// more folder does not drop the others.
    pub fn new(configured: &[FilingLocation]) -> Self {
        Self {
            chosen: configured.to_vec(),
            ..Self::default()
        }
    }

    fn archive_indices(&self) -> impl Iterator<Item = usize> + '_ {
        self.chosen
            .iter()
            .enumerate()
            .filter(|(_, location)| location.mode == LocationMode::Archive)
            .map(|(index, _)| index)
    }

    /// The document folder new documents are filed into: the only one, or the
    /// one marked. `None` while the user still has to choose.
    pub fn target_index(&self) -> Option<usize> {
        let mut archives = self.archive_indices();
        match (archives.next(), archives.next()) {
            (Some(only), None) => Some(only),
            _ => self
                .archive_indices()
                .find(|&index| self.chosen.get(index).is_some_and(|l| l.is_target)),
        }
    }

    /// From two document folders on the user picks the filing location.
    pub fn chooses_target(&self) -> bool {
        self.archive_indices().nth(1).is_some()
    }

    /// The list as it is written: the effective target is marked, nothing else.
    pub fn locations_to_save(&self) -> Vec<FilingLocation> {
        let target = self.target_index();
        self.chosen
            .iter()
            .enumerate()
            .map(|(index, location)| FilingLocation {
                is_target: Some(index) == target,
                transfer: Default::default(),
                ..location.clone()
            })
            .collect()
    }

    pub fn problem(&self) -> Option<SetupProblem> {
        if self.chosen.is_empty() {
            return None;
        }
        if self.archive_indices().next().is_none() {
            return Some(SetupProblem::NoDocumentFolder);
        }
        if self.chooses_target() && self.target_index().is_none() {
            return Some(SetupProblem::ChooseFilingLocation);
        }
        config::check_filing_rules(&self.locations_to_save())
            .err()
            .map(SetupProblem::Rule)
    }
}

/// A role the list would not allow, with the list it was tried on so the
/// problem can name the folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleRefused {
    pub problem: SetupProblem,
    pub chosen: Vec<FilingLocation>,
}

/// The list to write once `folder` plays `role`: a folder not listed yet is
/// added, a filing location takes the target over, and the list has to pass
/// the same checks as in the setup card.
pub fn locations_with_role(
    configured: &[FilingLocation],
    folder: &Path,
    role: FolderRole,
) -> Result<Vec<FilingLocation>, RoleRefused> {
    let (mode, is_target) = match role {
        FolderRole::Inbox => (LocationMode::Inbox, false),
        FolderRole::FilingLocation => (LocationMode::Archive, true),
        FolderRole::Archive => (LocationMode::Archive, false),
    };
    let mut setup = FilingSetup::new(configured);
    match setup.chosen.iter_mut().find(|l| l.path == folder) {
        Some(location) => {
            location.mode = mode;
            location.is_target = is_target;
        }
        None => setup.chosen.push(FilingLocation {
            path: folder.to_path_buf(),
            mode,
            images: false,
            is_target,
            transfer: Default::default(),
        }),
    }
    if is_target {
        for location in &mut setup.chosen {
            location.is_target = location.path == folder;
        }
    }
    match setup.problem() {
        Some(problem) => Err(RoleRefused {
            problem,
            chosen: setup.chosen,
        }),
        None => Ok(setup.locations_to_save()),
    }
}

/// Blocking. The canonical path of the folder a panel shows: the rules
/// compare canonical paths, and the intake works on them too.
pub fn resolve_folder(folder: &Path) -> Result<PathBuf, String> {
    let canonical = std::fs::canonicalize(folder).map_err(|err| err.to_string())?;
    if !canonical.is_dir() {
        return Err(std::io::Error::from(std::io::ErrorKind::NotADirectory).to_string());
    }
    Ok(canonical)
}

/// Blocking. Writes `chosen` to the config at `config_path` and returns the
/// filing settings read back from it. A location taken over from the config
/// whose folder cannot be reached right now (an unplugged drive) keeps the
/// path it had; every other one is written canonical.
pub fn write_locations(
    config_path: &Path,
    chosen: &[FilingLocation],
) -> Result<Filing, FilingWriteError> {
    let canonical: Vec<FilingLocation> = chosen
        .iter()
        .map(|location| FilingLocation {
            path: std::fs::canonicalize(&location.path).unwrap_or_else(|_| location.path.clone()),
            ..location.clone()
        })
        .collect();
    config::write_filing_locations(config_path, &canonical).map(|config| config.filing)
}

// reason: a failing assertion is the signal in a test
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        PathBuf::from("/home/user").join(name)
    }

    fn location(mode: LocationMode, name: &str, is_target: bool) -> FilingLocation {
        location_at(folder(name), mode, is_target)
    }

    fn location_at(path: PathBuf, mode: LocationMode, is_target: bool) -> FilingLocation {
        FilingLocation {
            path,
            mode,
            images: false,
            is_target,
            transfer: Default::default(),
        }
    }

    #[test]
    fn the_setup_starts_from_the_configured_locations() {
        let configured = [location(LocationMode::Archive, "filed", true)];
        assert_eq!(FilingSetup::new(&configured).chosen, configured);
    }

    #[test]
    fn a_config_from_the_first_setup_loads_and_saves_back_with_the_same_meaning() {
        let base = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(base.path()).unwrap();
        for name in ["tubes", "records", "filed"] {
            std::fs::create_dir(root.join(name)).unwrap();
        }
        let config_path = root.join("config.toml");
        let entry = |name: &str, rest: &str| {
            format!(
                "[[modules.filing.locations]]\npath = '{}'\n{rest}\n",
                root.join(name).display()
            )
        };
        let text = format!(
            "[modules.filing]\nenabled = true\n\n{}\n{}\n{}",
            entry("tubes", "mode = \"inbox\"\nimages = true"),
            entry("records", "mode = \"archive\""),
            entry("filed", "mode = \"archive\"\ntarget = true"),
        );
        std::fs::write(&config_path, text).unwrap();

        let first = config::load(&config_path).unwrap().filing.locations;
        let setup = FilingSetup::new(&first);
        assert_eq!(setup.problem(), None);
        assert_eq!(setup.target_index(), Some(2));
        assert_eq!(first[2].path, root.join("filed"));

        let list = setup.locations_to_save();
        write_locations(&config_path, &list).unwrap();
        let second = config::load(&config_path).unwrap().filing.locations;
        assert_eq!(second, first);
        assert!(first[0].images && first[2].is_target);
    }

    #[test]
    fn a_folder_is_resolved_to_its_canonical_path() {
        let base = tempfile::tempdir().unwrap();
        let real = base.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let file = base.path().join("file.txt");
        std::fs::write(&file, "").unwrap();

        let spelled_round_about = base.path().join("real").join(".");
        assert_eq!(
            resolve_folder(&spelled_round_about).unwrap(),
            std::fs::canonicalize(&real).unwrap()
        );
        resolve_folder(&file).unwrap_err();
        resolve_folder(&base.path().join("missing")).unwrap_err();
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_reached_through_a_symlink_is_written_by_its_real_path() {
        let base = tempfile::tempdir().unwrap();
        let real = base.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = base.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let config_path = base.path().join("config.toml");

        let filing = write_locations(
            &config_path,
            &[location_at(link.clone(), LocationMode::Archive, true)],
        )
        .unwrap();

        assert_eq!(
            filing.locations[0].path,
            std::fs::canonicalize(&real).unwrap()
        );
        assert!(filing.enabled);
    }

    #[cfg(unix)]
    #[test]
    fn two_spellings_of_one_folder_are_caught_before_writing() {
        let base = tempfile::tempdir().unwrap();
        let real = base.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = base.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let config_path = base.path().join("config.toml");

        let result = write_locations(
            &config_path,
            &[
                location_at(real, LocationMode::Archive, true),
                location_at(link, LocationMode::Inbox, false),
            ],
        );

        assert_eq!(
            result,
            Err(FilingWriteError::Rule(FilingRuleViolation::Overlap {
                earlier: 0,
                later: 1
            }))
        );
        assert!(!config_path.exists());
    }

    #[test]
    fn paths_below_home_are_shown_with_a_tilde() {
        let home = Path::new("/home/user");
        assert_eq!(shown_path(home, home), "~");
        assert_eq!(
            shown_path(&home.join("Documents"), home),
            format!("~{}Documents", std::path::MAIN_SEPARATOR)
        );
        assert_eq!(shown_path(Path::new("/srv/files"), home), "/srv/files");
    }

    fn configured() -> Vec<FilingLocation> {
        vec![
            location(LocationMode::Inbox, "Scans", false),
            location(LocationMode::Archive, "Filed", true),
            location(LocationMode::Archive, "Old", false),
        ]
    }

    #[test]
    fn a_filing_location_takes_the_target_over() {
        let list =
            locations_with_role(&configured(), &folder("Old"), FolderRole::FilingLocation).unwrap();
        let targets: Vec<_> = list.iter().filter(|l| l.is_target).collect();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].path, folder("Old"));
    }

    #[test]
    fn an_inbox_is_never_the_target_and_the_target_is_not_left_open() {
        let list = locations_with_role(&configured(), &folder("Filed"), FolderRole::Inbox).unwrap();
        assert!(list
            .iter()
            .all(|l| l.mode == LocationMode::Archive || !l.is_target));
        assert_eq!(list.iter().filter(|l| l.is_target).count(), 1);

        let mut three = configured();
        three.push(location(LocationMode::Archive, "Third", false));
        let refused = locations_with_role(&three, &folder("Filed"), FolderRole::Inbox).unwrap_err();
        assert_eq!(refused.problem, SetupProblem::ChooseFilingLocation);
    }

    #[test]
    fn a_new_folder_is_added_with_its_role() {
        let list = locations_with_role(&configured(), &folder("New"), FolderRole::Inbox).unwrap();
        assert_eq!(list.len(), 4);
        assert_eq!(list[3].mode, LocationMode::Inbox);
        let inside = folder("Filed").join("2025");
        assert!(matches!(
            locations_with_role(&configured(), &inside, FolderRole::Inbox)
                .unwrap_err()
                .problem,
            SetupProblem::Rule(FilingRuleViolation::Overlap { .. })
        ));
    }
}
