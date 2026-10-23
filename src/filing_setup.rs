//! The filing setup: the user walks the panels to a folder, adds it as an
//! inbox, an archive or the target, and saves the list to `[modules.filing]`
//! in the config file.
//!
//! The state only. Looking up a folder and writing the file touch the disk
//! and run on a blocking thread; the app hands their results back.

use std::path::{Path, PathBuf};

use ncrs_intake::LocationMode;

use crate::config::{self, Filing, FilingLocation, FilingRuleViolation, FilingWriteError};

/// What a folder is added as. The target is an archive the config marks with
/// `target = true`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocationKind {
    Inbox,
    Archive,
    Target,
}

impl LocationKind {
    pub fn of(location: &FilingLocation) -> Self {
        match (location.mode, location.is_target) {
            (LocationMode::Inbox, _) => Self::Inbox,
            (LocationMode::Archive, true) => Self::Target,
            (LocationMode::Archive, false) => Self::Archive,
        }
    }

    fn location(self, path: PathBuf) -> FilingLocation {
        let (mode, is_target) = match self {
            Self::Inbox => (LocationMode::Inbox, false),
            Self::Archive => (LocationMode::Archive, false),
            Self::Target => (LocationMode::Archive, true),
        };
        FilingLocation {
            path,
            mode,
            images: false,
            is_target,
        }
    }
}

/// What the card says about the last thing the user did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupNote {
    FolderUnusable {
        folder: PathBuf,
        reason: String,
    },
    NoConfigPath,
    SaveFailed(FilingWriteError),
    /// The module was started with the saved locations.
    Applied,
    /// A module with locations is already running; it keeps them until the
    /// next start.
    AppliesAtNextStart,
}

#[derive(Debug, Default)]
pub struct FilingSetup {
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

    /// The first rule the list breaks. Shown at once, so the user sees what
    /// to fix before saving.
    pub fn violation(&self) -> Option<FilingRuleViolation> {
        config::check_filing_rules(&self.chosen).err()
    }

    pub fn can_save(&self) -> bool {
        !self.chosen.is_empty() && self.violation().is_none() && !self.is_busy()
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

    pub fn finish_adding(
        &mut self,
        kind: LocationKind,
        folder: PathBuf,
        resolved: Result<PathBuf, String>,
    ) {
        self.adding = false;
        match resolved {
            Ok(canonical) => self.chosen.push(kind.location(canonical)),
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
        Some(self.chosen.clone())
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

    fn added(setup: &mut FilingSetup, kind: LocationKind, name: &str) {
        assert!(setup.start_adding());
        setup.finish_adding(kind, folder(name), Ok(folder(name)));
    }

    #[test]
    fn each_kind_becomes_its_mode_and_target_flag() {
        let mut setup = FilingSetup::new(&[]);
        added(&mut setup, LocationKind::Inbox, "inbox");
        added(&mut setup, LocationKind::Archive, "archive");
        added(&mut setup, LocationKind::Target, "target");

        let found: Vec<_> = setup
            .chosen()
            .iter()
            .map(|location| {
                (
                    location.mode,
                    location.is_target,
                    LocationKind::of(location),
                )
            })
            .collect();
        assert_eq!(
            found,
            [
                (LocationMode::Inbox, false, LocationKind::Inbox),
                (LocationMode::Archive, false, LocationKind::Archive),
                (LocationMode::Archive, true, LocationKind::Target),
            ]
        );
        assert_eq!(setup.violation(), None);
        assert!(setup.can_save());
    }

    #[test]
    fn a_broken_rule_shows_at_once_and_blocks_saving() {
        let mut setup = FilingSetup::new(&[]);
        assert!(!setup.can_save(), "an empty list has nothing to save");
        added(&mut setup, LocationKind::Inbox, "inbox");
        assert_eq!(setup.violation(), Some(FilingRuleViolation::NoTarget));
        added(&mut setup, LocationKind::Target, "target");
        added(&mut setup, LocationKind::Target, "second");
        assert_eq!(
            setup.violation(),
            Some(FilingRuleViolation::SecondTarget { index: 2 })
        );
        assert_eq!(setup.start_saving(), None);

        setup.remove(2);
        added(&mut setup, LocationKind::Archive, "target/inside");
        assert_eq!(
            setup.violation(),
            Some(FilingRuleViolation::Overlap {
                earlier: 1,
                later: 2
            })
        );
        setup.remove(2);
        assert!(setup.start_saving().is_some());
    }

    #[test]
    fn images_toggle_and_entries_go_per_location() {
        let mut setup = FilingSetup::new(&[]);
        added(&mut setup, LocationKind::Inbox, "inbox");
        added(&mut setup, LocationKind::Target, "target");

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
        assert_eq!(setup.chosen()[0].path, folder("target"));
        setup.remove(5);
        assert_eq!(setup.chosen().len(), 1);
    }

    #[test]
    fn nothing_changes_while_a_lookup_or_the_save_runs() {
        let mut setup = FilingSetup::new(&[]);
        added(&mut setup, LocationKind::Target, "target");
        assert!(setup.start_adding());
        assert!(!setup.start_adding());
        setup.remove(0);
        setup.toggle_images(0);
        assert_eq!(setup.start_saving(), None);
        assert_eq!(setup.chosen().len(), 1);
        assert!(!setup.chosen()[0].images);

        setup.finish_adding(
            LocationKind::Inbox,
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
        let configured = [LocationKind::Target.location(folder("target"))];
        assert_eq!(FilingSetup::new(&configured).chosen(), configured);
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

        let filing =
            write_locations(&config_path, &[LocationKind::Target.location(link.clone())]).unwrap();

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
                LocationKind::Target.location(real),
                LocationKind::Inbox.location(link),
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
}
