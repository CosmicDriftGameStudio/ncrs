//! The filing setup: the user adds folders as Tubes (inboxes) and as document
//! folders (archives), marks which document folder is the Central Filing, and
//! saves the list to `[modules.filing]` in the config file.
//!
//! The state only. Looking up a folder and writing the file touch the disk
//! and run on a blocking thread; the app hands their results back.

use std::path::{Path, PathBuf};

use iced::Task;
use ncrs_intake::LocationMode;

use crate::config::{self, Filing, FilingLocation, FilingRuleViolation, FilingWriteError};
use crate::hermes_column::FolderRole;

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
}

/// What keeps the list from being saved. Shown at once, so the user sees what
/// to fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupProblem {
    NoDocumentFolder,
    ChooseCentralFiling,
    Rule(FilingRuleViolation),
}

#[derive(Debug, Default)]
pub struct FilingSetup {
    /// In the order the folders were added. Every index the card sends points
    /// into this list, whichever section shows the entry.
    chosen: Vec<FilingLocation>,
    adding: bool,
    saving: bool,
    note: Option<SetupNote>,
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

    pub fn chosen(&self) -> &[FilingLocation] {
        &self.chosen
    }

    pub fn note(&self) -> Option<&SetupNote> {
        self.note.as_ref()
    }

    pub fn is_busy(&self) -> bool {
        self.adding || self.saving
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

    /// From two document folders on the user picks the Central Filing.
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
            return Some(SetupProblem::ChooseCentralFiling);
        }
        config::check_filing_rules(&self.locations_to_save())
            .err()
            .map(SetupProblem::Rule)
    }

    pub fn can_save(&self) -> bool {
        !self.chosen.is_empty() && self.problem().is_none() && !self.is_busy()
    }

    /// `false` while another lookup or the save is running.
    pub fn start_adding(&mut self) -> bool {
        if self.is_busy() {
            return false;
        }
        self.adding = true;
        self.note = None;
        true
    }

    /// The dialog was cancelled or could not open.
    pub fn cancel_adding(&mut self) {
        self.adding = false;
    }

    pub fn finish_adding(
        &mut self,
        mode: LocationMode,
        folder: PathBuf,
        resolved: Result<PathBuf, String>,
    ) {
        if !self.adding {
            return;
        }
        self.adding = false;
        match resolved {
            Ok(canonical) if self.chosen.iter().any(|l| l.path == canonical) => {
                self.note = Some(SetupNote::AlreadyAdded { folder: canonical });
            }
            Ok(canonical) => self.chosen.push(FilingLocation {
                path: canonical,
                mode,
                images: false,
                is_target: false,
            }),
            Err(reason) => self.note = Some(SetupNote::FolderUnusable { folder, reason }),
        }
    }

    pub fn toggle_images(&mut self, index: usize) {
        if self.is_busy() {
            return;
        }
        if let Some(location) = self.chosen.get_mut(index) {
            location.images = !location.images;
            self.note = None;
        }
    }

    /// Marks the document folder at `index` as the Central Filing.
    pub fn set_target(&mut self, index: usize) {
        let is_archive = self
            .chosen
            .get(index)
            .is_some_and(|location| location.mode == LocationMode::Archive);
        if self.is_busy() || !is_archive {
            return;
        }
        for (at, location) in self.chosen.iter_mut().enumerate() {
            location.is_target = at == index;
        }
        self.note = None;
    }

    pub fn remove(&mut self, index: usize) {
        if self.is_busy() || index >= self.chosen.len() {
            return;
        }
        self.chosen.remove(index);
        self.note = None;
    }

    /// The list to write, or `None` when it cannot be saved now.
    pub fn start_saving(&mut self) -> Option<Vec<FilingLocation>> {
        if !self.can_save() {
            return None;
        }
        self.saving = true;
        self.note = None;
        Some(self.locations_to_save())
    }

    pub fn finish_saving(&mut self, note: SetupNote) {
        self.saving = false;
        self.note = Some(note);
    }

    /// For a failure before the save could start, such as a missing config path.
    pub fn refuse_saving(&mut self, note: SetupNote) {
        self.note = Some(note);
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

/// Opens the system folder dialog in `start_in` and yields the folder picked,
/// or `None` when it was cancelled.
pub fn pick_folder_with_system_dialog(start_in: &Path) -> Task<Option<PathBuf>> {
    // `pick_folder()` runs here, not inside the task: on macOS it sets up the
    // panel, which AppKit allows only on the main thread, and this is called
    // from `update`.
    let picked = rfd::AsyncFileDialog::new()
        .set_directory(start_in)
        .pick_folder();
    Task::perform(picked, |chosen| {
        chosen.map(|handle| handle.path().to_path_buf())
    })
}

/// What the file dialog of the settings tab picks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilePick {
    /// The program behind F3 or F4; only app bundles in the App Store build.
    Program { apps_only: bool },
    /// A VS Code color theme.
    ThemeFile,
}

/// Opens the system file dialog for `pick` and yields the file picked, or
/// `None` when it was cancelled. Called from `update`, like
/// [`pick_folder_with_system_dialog`].
pub fn pick_file_with_system_dialog(pick: FilePick) -> Task<Option<PathBuf>> {
    let plain_dialog = rfd::AsyncFileDialog::new();
    let dialog = match pick {
        FilePick::Program { apps_only } => {
            let in_apps = if cfg!(target_os = "macos") {
                plain_dialog.set_directory("/Applications")
            } else {
                plain_dialog
            };
            if apps_only {
                in_apps.add_filter("App", &["app"])
            } else {
                in_apps
            }
        }
        FilePick::ThemeFile => plain_dialog.add_filter("JSON", &["json"]),
    };
    Task::perform(dialog.pick_file(), |chosen| {
        chosen.map(|handle| handle.path().to_path_buf())
    })
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

/// `~` for the home directory and what lies below it, so the card shows
/// short paths.
pub fn shown_path(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Ok(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        Err(_) => path.display().to_string(),
    }
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
        }
    }

    fn added(setup: &mut FilingSetup, mode: LocationMode, name: &str) {
        assert!(setup.start_adding());
        setup.finish_adding(mode, folder(name), Ok(folder(name)));
    }

    #[test]
    fn one_document_folder_is_the_central_filing_without_a_choice() {
        let mut setup = FilingSetup::new(&[]);
        added(&mut setup, LocationMode::Archive, "filed");

        assert!(!setup.chooses_target());
        assert_eq!(setup.target_index(), Some(0));
        assert_eq!(setup.problem(), None);
        assert!(setup.can_save());
        let saved = setup.start_saving().unwrap();
        assert_eq!(saved.len(), 1);
        assert!(saved[0].is_target);

        let mut only_a_tube = FilingSetup::new(&[]);
        added(&mut only_a_tube, LocationMode::Inbox, "tubes");
        assert_eq!(only_a_tube.problem(), Some(SetupProblem::NoDocumentFolder));
        assert!(!only_a_tube.can_save());
        assert_eq!(only_a_tube.start_saving(), None);
    }

    #[test]
    fn two_document_folders_need_a_choice_before_saving() {
        let mut setup = FilingSetup::new(&[]);
        added(&mut setup, LocationMode::Inbox, "tubes");
        added(&mut setup, LocationMode::Archive, "records");
        added(&mut setup, LocationMode::Archive, "filed");

        assert!(setup.chooses_target());
        assert_eq!(setup.target_index(), None);
        assert_eq!(setup.problem(), Some(SetupProblem::ChooseCentralFiling));
        assert_eq!(setup.start_saving(), None);

        setup.set_target(0);
        assert_eq!(setup.target_index(), None, "a tube cannot be the target");
        assert_eq!(setup.problem(), Some(SetupProblem::ChooseCentralFiling));

        setup.set_target(2);
        assert!(setup.can_save());
        let marks: Vec<bool> = setup
            .start_saving()
            .unwrap()
            .iter()
            .map(|location| location.is_target)
            .collect();
        assert_eq!(marks, [false, false, true]);
    }

    #[test]
    fn removing_the_marked_folder_asks_for_a_choice_again() {
        let mut setup = FilingSetup::new(&[]);
        added(&mut setup, LocationMode::Archive, "a");
        added(&mut setup, LocationMode::Archive, "b");
        added(&mut setup, LocationMode::Archive, "c");
        setup.set_target(1);
        assert!(setup.can_save());

        setup.remove(1);
        assert_eq!(setup.problem(), Some(SetupProblem::ChooseCentralFiling));

        setup.remove(0);
        assert!(!setup.chooses_target());
        assert_eq!(setup.target_index(), Some(0));
        assert!(setup.can_save());
    }

    #[test]
    fn a_configured_target_keeps_its_mark_when_another_folder_is_added() {
        let configured = [location(LocationMode::Archive, "filed", true)];
        let mut setup = FilingSetup::new(&configured);
        added(&mut setup, LocationMode::Archive, "records");

        assert!(setup.chooses_target());
        assert_eq!(setup.target_index(), Some(0));
        assert!(setup.can_save());
    }

    #[test]
    fn the_same_folder_twice_is_a_note_and_adds_nothing() {
        let mut setup = FilingSetup::new(&[]);
        added(&mut setup, LocationMode::Inbox, "tubes");
        added(&mut setup, LocationMode::Archive, "filed");

        added(&mut setup, LocationMode::Archive, "filed");
        assert_eq!(setup.chosen().len(), 2);
        assert_eq!(
            setup.note(),
            Some(&SetupNote::AlreadyAdded {
                folder: folder("filed")
            })
        );

        added(&mut setup, LocationMode::Archive, "tubes");
        assert_eq!(setup.chosen().len(), 2);
        assert_eq!(setup.problem(), None);
    }

    #[test]
    fn a_cancelled_dialog_adds_nothing_and_a_stray_result_is_ignored() {
        let mut setup = FilingSetup::new(&[]);
        assert!(setup.start_adding());
        assert!(setup.is_busy());
        setup.cancel_adding();
        assert!(!setup.is_busy());
        assert!(setup.chosen().is_empty());

        setup.finish_adding(LocationMode::Inbox, folder("x"), Ok(folder("x")));
        assert!(setup.chosen().is_empty());
    }

    #[test]
    fn a_tube_inside_the_document_folder_is_an_overlap() {
        let mut setup = FilingSetup::new(&[]);
        added(&mut setup, LocationMode::Archive, "filed");
        added(&mut setup, LocationMode::Inbox, "filed/scanner");
        assert_eq!(
            setup.problem(),
            Some(SetupProblem::Rule(FilingRuleViolation::Overlap {
                earlier: 0,
                later: 1
            }))
        );
        assert_eq!(setup.start_saving(), None);
    }

    #[test]
    fn images_toggle_and_entries_go_per_location() {
        let mut setup = FilingSetup::new(&[]);
        added(&mut setup, LocationMode::Inbox, "inbox");
        added(&mut setup, LocationMode::Archive, "filed");

        setup.toggle_images(1);
        assert_eq!(
            setup
                .chosen()
                .iter()
                .map(|location| location.images)
                .collect::<Vec<_>>(),
            [false, true]
        );
        setup.remove(0);
        assert_eq!(setup.chosen().len(), 1);
        assert_eq!(setup.chosen()[0].path, folder("filed"));
        setup.remove(5);
        assert_eq!(setup.chosen().len(), 1);
    }

    #[test]
    fn nothing_changes_while_a_lookup_or_the_save_runs() {
        let mut setup = FilingSetup::new(&[]);
        added(&mut setup, LocationMode::Archive, "filed");
        added(&mut setup, LocationMode::Archive, "records");
        setup.set_target(0);
        assert!(setup.start_adding());
        assert!(!setup.start_adding());
        setup.remove(0);
        setup.toggle_images(0);
        setup.set_target(1);
        assert_eq!(setup.start_saving(), None);
        assert_eq!(setup.chosen().len(), 2);
        assert!(!setup.chosen()[0].images);
        assert_eq!(setup.target_index(), Some(0));

        setup.finish_adding(
            LocationMode::Inbox,
            folder("gone"),
            Err("not found".to_owned()),
        );
        assert_eq!(
            setup.note(),
            Some(&SetupNote::FolderUnusable {
                folder: folder("gone"),
                reason: "not found".to_owned()
            })
        );
        assert!(setup.start_saving().is_some());
        assert!(!setup.start_adding());
    }

    #[test]
    fn the_setup_starts_from_the_configured_locations() {
        let configured = [location(LocationMode::Archive, "filed", true)];
        assert_eq!(FilingSetup::new(&configured).chosen(), configured);
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
        let mut setup = FilingSetup::new(&first);
        assert_eq!(setup.problem(), None);
        assert_eq!(setup.target_index(), Some(2));
        assert_eq!(first[2].path, root.join("filed"));

        let list = setup.start_saving().unwrap();
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
        assert_eq!(refused.problem, SetupProblem::ChooseCentralFiling);
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
