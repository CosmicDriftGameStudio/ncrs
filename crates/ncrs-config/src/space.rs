//! The `[space]` section: what "Platz schaffen" treats as safe and what it never touches.

use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const DEFAULT_WARN_FROM_PERCENT: u8 = 90;

/// The catalog name (`ncrs_space::CatalogId::name`) of the node_modules of projects not
/// changed for a long time. Off until the user switches it on.
pub const NODE_MODULES_STALE: &str = "node-modules-stale";

/// How deep the search for big folders of your own goes below home.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Depth {
    #[default]
    Auto,
    Levels(u8),
}

/// What the Platz tab reads from `[space]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceSettings {
    /// The drive counts as full from this used share on.
    pub warn_from_percent: u8,
    /// Names (volume labels) of the drives the warning is for. `None`: the drive of the home
    /// folder only; `Some` of an empty list: no drive.
    pub warn_drives: Option<Vec<String>>,
    /// Catalog entries switched off, by their name in the catalog (`ncrs_space::CatalogId::name`).
    pub catalog_off: Vec<String>,
    /// Folders of the user's own that a tool regenerates; treated as safe.
    pub own_regenerable: Vec<PathBuf>,
    pub never_touch: Vec<PathBuf>,
    pub show_worktrees: bool,
    pub depth: Depth,
    pub other_drives: bool,
}

impl Default for SpaceSettings {
    fn default() -> Self {
        Self {
            warn_from_percent: DEFAULT_WARN_FROM_PERCENT,
            warn_drives: None,
            catalog_off: vec![NODE_MODULES_STALE.to_owned()],
            own_regenerable: Vec::new(),
            never_touch: Vec::new(),
            show_worktrees: true,
            depth: Depth::Auto,
            other_drives: false,
        }
    }
}

/// The depths the settings page offers besides "auto".
pub const DEPTH_CHOICES: [u8; 3] = [2, 3, 4];
/// The range the drive-full warning takes; outside it is the default.
pub const WARN_FROM_RANGE: std::ops::RangeInclusive<u8> = 50..=99;

/// The `[space]` section as written, unchecked.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpaceSection {
    /// Checked by `resolve_space`: outside [`WARN_FROM_RANGE`] is the default.
    #[serde(default)]
    pub warn_from_percent: Option<i64>,
    #[serde(default)]
    pub warn_drives: Option<Vec<String>>,
    #[serde(default)]
    pub catalog_off: Option<Vec<String>>,
    #[serde(default)]
    pub own_regenerable: Option<Vec<PathBuf>>,
    #[serde(default)]
    pub never_touch: Option<Vec<PathBuf>>,
    #[serde(default)]
    pub show_worktrees: Option<bool>,
    /// `"auto"` or one of [`DEPTH_CHOICES`]; anything else is auto.
    #[serde(default)]
    pub depth: Option<DepthValue>,
    #[serde(default)]
    pub other_drives: Option<bool>,
}

/// `depth` as the file may write it: a word or a number.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum DepthValue {
    Word(String),
    Levels(i64),
}

/// The folders "Platz schaffen" leaves alone until the user says otherwise: backups,
/// documents and the cloud folders.
pub fn default_never_touch(home: &Path) -> Vec<PathBuf> {
    let mut folders = vec![home.join("Backups"), home.join("Documents")];
    folders.extend(cloud_folders(home));
    folders
}

/// Names in the home folder that start like these are sync roots too (`OneDrive - Company`).
const CLOUD_PREFIXES: [&str; 2] = ["OneDrive", "Dropbox"];
/// The variables the OneDrive client sets to its sync roots.
const ONEDRIVE_VARIABLES: [&str; 3] = ["OneDrive", "OneDriveCommercial", "OneDriveConsumer"];

/// Where iCloud, Dropbox and OneDrive keep their files, whether or not the service is installed:
/// the fixed places, the folders of home that start like a sync root and the OneDrive variables.
pub fn cloud_folders(home: &Path) -> Vec<PathBuf> {
    let names: Vec<String> = std::fs::read_dir(home)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .collect();
    let from_environment: Vec<PathBuf> = ONEDRIVE_VARIABLES
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .filter(|folder| !folder.as_os_str().is_empty())
        .collect();
    cloud_folders_in(home, &names, &from_environment)
}

fn cloud_folders_in(
    home: &Path,
    home_names: &[String],
    from_environment: &[PathBuf],
) -> Vec<PathBuf> {
    let mut folders = Vec::new();
    if cfg!(target_os = "macos") {
        folders.push(home.join("Library/Mobile Documents"));
        folders.push(home.join("Library/CloudStorage"));
    }
    if cfg!(windows) {
        folders.push(home.join("iCloudDrive"));
    }
    folders.push(home.join("Dropbox"));
    folders.push(home.join("OneDrive"));
    let by_prefix = home_names
        .iter()
        .filter(|name| CLOUD_PREFIXES.iter().any(|prefix| name.starts_with(prefix)))
        .map(|name| home.join(name));
    for folder in by_prefix.chain(from_environment.iter().cloned()) {
        if !folders.contains(&folder) {
            folders.push(folder);
        }
    }
    folders
}

/// Whether `folder` is one of [`cloud_folders`], so a list can show them as one entry.
pub fn is_cloud_folder(folder: &Path, home: &Path) -> bool {
    cloud_folders(home).iter().any(|cloud| cloud == folder)
}

/// Whether the drive-full warning is for the drive called `name`.
pub fn warns_for_drive(settings: &SpaceSettings, name: &str, is_home_drive: bool) -> bool {
    match &settings.warn_drives {
        None => is_home_drive,
        Some(listed) => listed.iter().any(|drive| drive == name),
    }
}

/// Checks `section`: a value out of range is the default, empty list entries are dropped.
/// A `never_touch` that is not written is [`default_never_touch`] of `home`.
pub fn resolve_space_at(section: &SpaceSection, home: Option<&Path>) -> SpaceSettings {
    let defaults = SpaceSettings {
        never_touch: home.map(default_never_touch).unwrap_or_default(),
        ..SpaceSettings::default()
    };
    let warn_from_percent = section
        .warn_from_percent
        .and_then(|percent| u8::try_from(percent).ok())
        .filter(|percent| WARN_FROM_RANGE.contains(percent))
        .unwrap_or(defaults.warn_from_percent);
    let depth = match &section.depth {
        Some(DepthValue::Levels(levels)) => u8::try_from(*levels)
            .ok()
            .filter(|depth| DEPTH_CHOICES.contains(depth))
            .map_or(Depth::Auto, Depth::Levels),
        Some(DepthValue::Word(_)) | None => Depth::Auto,
    };
    let folders = |listed: &Option<Vec<PathBuf>>| {
        listed.as_ref().map(|folders| {
            folders
                .iter()
                .filter(|folder| !folder.as_os_str().is_empty())
                .cloned()
                .collect()
        })
    };
    SpaceSettings {
        warn_from_percent,
        warn_drives: section
            .warn_drives
            .as_ref()
            .map_or(defaults.warn_drives, |listed| {
                Some(
                    listed
                        .iter()
                        .map(|name| name.trim().to_owned())
                        .filter(|name| !name.is_empty())
                        .collect(),
                )
            }),
        catalog_off: section
            .catalog_off
            .as_ref()
            .map_or(defaults.catalog_off, |listed| {
                listed
                    .iter()
                    .map(|name| name.trim().to_owned())
                    .filter(|name| !name.is_empty())
                    .collect()
            }),
        own_regenerable: folders(&section.own_regenerable).unwrap_or(defaults.own_regenerable),
        never_touch: folders(&section.never_touch).unwrap_or(defaults.never_touch),
        show_worktrees: section.show_worktrees.unwrap_or(defaults.show_worktrees),
        depth,
        other_drives: section.other_drives.unwrap_or(defaults.other_drives),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_written_is_the_defaults() {
        assert_eq!(
            resolve_space_at(&SpaceSection::default(), None),
            SpaceSettings::default()
        );
    }

    #[test]
    fn the_stale_node_modules_are_off_by_default() {
        assert_eq!(
            SpaceSettings::default().catalog_off,
            vec![NODE_MODULES_STALE.to_owned()]
        );
    }

    #[test]
    fn never_touch_defaults_to_backups_documents_and_the_cloud_folders() {
        let home = Path::new("/home/a");
        let settings = resolve_space_at(&SpaceSection::default(), Some(home));
        assert!(settings.never_touch.contains(&home.join("Backups")));
        assert!(settings.never_touch.contains(&home.join("Documents")));
        assert!(settings.never_touch.contains(&home.join("Dropbox")));
        assert!(settings.never_touch.contains(&home.join("OneDrive")));
        assert!(settings
            .never_touch
            .iter()
            .all(|folder| folder.starts_with(home)));
    }

    #[test]
    fn a_written_never_touch_replaces_the_defaults_even_when_empty() {
        let home = Path::new("/home/a");
        let section = SpaceSection {
            never_touch: Some(Vec::new()),
            ..SpaceSection::default()
        };
        assert!(resolve_space_at(&section, Some(home))
            .never_touch
            .is_empty());
    }

    #[test]
    fn a_cloud_folder_is_recognised() {
        let home = Path::new("/home/a");
        assert!(is_cloud_folder(&home.join("Dropbox"), home));
        assert!(!is_cloud_folder(&home.join("Documents"), home));
    }

    #[test]
    fn the_warning_is_for_the_home_drive_unless_drives_are_listed() {
        let mut settings = SpaceSettings::default();
        assert!(warns_for_drive(&settings, "Macintosh HD", true));
        assert!(!warns_for_drive(&settings, "Archiv", false));
        settings.warn_drives = Some(vec!["Archiv".to_owned()]);
        assert!(!warns_for_drive(&settings, "Macintosh HD", true));
        assert!(warns_for_drive(&settings, "Archiv", false));
    }

    #[test]
    fn an_explicit_empty_list_turns_the_warning_off_for_every_drive() {
        let section = SpaceSection {
            warn_drives: Some(Vec::new()),
            ..SpaceSection::default()
        };
        let settings = resolve_space_at(&section, None);
        assert_eq!(settings.warn_drives, Some(Vec::new()));
        assert!(!warns_for_drive(&settings, "Macintosh HD", true));
    }

    #[test]
    fn cloud_folders_cover_onedrive_for_business_and_the_environment_paths() {
        let home = Path::new("/home/a");
        let folders = cloud_folders_in(
            home,
            &[
                "OneDrive - Org".to_owned(),
                "Dropbox (Team)".to_owned(),
                "Music".to_owned(),
            ],
            &[PathBuf::from("/data/OneDriveCommercial")],
        );
        assert!(folders.contains(&home.join("OneDrive - Org")));
        assert!(folders.contains(&home.join("Dropbox (Team)")));
        assert!(!folders.contains(&home.join("Music")));
        assert!(folders.contains(&PathBuf::from("/data/OneDriveCommercial")));
    }

    #[test]
    fn listed_drives_are_trimmed_and_empty_names_dropped() {
        let section = SpaceSection {
            warn_drives: Some(vec![" Archiv ".to_owned(), String::new()]),
            ..SpaceSection::default()
        };
        assert_eq!(
            resolve_space_at(&section, None).warn_drives,
            Some(vec!["Archiv".to_owned()])
        );
    }

    #[test]
    fn the_warning_share_outside_50_to_99_is_the_default() {
        for written in [0, 49, 100, -3, 500] {
            let section = SpaceSection {
                warn_from_percent: Some(written),
                ..SpaceSection::default()
            };
            assert_eq!(
                resolve_space_at(&section, None).warn_from_percent,
                DEFAULT_WARN_FROM_PERCENT
            );
        }
        for written in [50, 75, 99] {
            let section = SpaceSection {
                warn_from_percent: Some(i64::from(written)),
                ..SpaceSection::default()
            };
            assert_eq!(resolve_space_at(&section, None).warn_from_percent, written);
        }
    }

    #[test]
    fn depth_is_auto_or_two_to_four() {
        let depth_of = |value: Option<DepthValue>| {
            resolve_space_at(
                &SpaceSection {
                    depth: value,
                    ..SpaceSection::default()
                },
                None,
            )
            .depth
        };
        assert_eq!(depth_of(None), Depth::Auto);
        assert_eq!(depth_of(Some(DepthValue::Word("auto".into()))), Depth::Auto);
        assert_eq!(depth_of(Some(DepthValue::Levels(3))), Depth::Levels(3));
        assert_eq!(depth_of(Some(DepthValue::Levels(1))), Depth::Auto);
        assert_eq!(depth_of(Some(DepthValue::Levels(9))), Depth::Auto);
    }

    #[test]
    fn empty_list_entries_are_dropped() {
        let section = SpaceSection {
            catalog_off: Some(vec![" Yarn ".to_owned(), String::new()]),
            never_touch: Some(vec![PathBuf::new(), PathBuf::from("/keep")]),
            ..SpaceSection::default()
        };
        let settings = resolve_space_at(&section, None);
        assert_eq!(settings.catalog_off, vec!["Yarn".to_owned()]);
        assert_eq!(settings.never_touch, vec![PathBuf::from("/keep")]);
    }
}
