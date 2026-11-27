//! The `[space]` section: what "Platz schaffen" treats as safe and what it never touches.

use std::path::PathBuf;

use serde::Deserialize;

pub const DEFAULT_WARN_FROM_PERCENT: u8 = 90;

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
            catalog_off: Vec::new(),
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

/// Checks `section`: a value out of range is the default, empty list entries are dropped.
pub fn resolve_space(section: &SpaceSection) -> SpaceSettings {
    let defaults = SpaceSettings::default();
    let warn_from_percent = section
        .warn_from_percent
        .and_then(|percent| u8::try_from(percent).ok())
        .filter(|percent| WARN_FROM_RANGE.contains(percent))
        .unwrap_or(defaults.warn_from_percent);
    let depth = match &section.depth {
        Some(DepthValue::Levels(levels)) => u8::try_from(*levels)
            .ok()
            .filter(|levels| DEPTH_CHOICES.contains(levels))
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
            resolve_space(&SpaceSection::default()),
            SpaceSettings::default()
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
                resolve_space(&section).warn_from_percent,
                DEFAULT_WARN_FROM_PERCENT
            );
        }
        for written in [50_u8, 75, 99] {
            let section = SpaceSection {
                warn_from_percent: Some(i64::from(written)),
                ..SpaceSection::default()
            };
            assert_eq!(resolve_space(&section).warn_from_percent, written);
        }
    }

    #[test]
    fn depth_is_auto_or_two_to_four() {
        let depth_of = |value: Option<DepthValue>| {
            resolve_space(&SpaceSection {
                depth: value,
                ..SpaceSection::default()
            })
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
        let settings = resolve_space(&section);
        assert_eq!(settings.catalog_off, vec!["Yarn".to_owned()]);
        assert_eq!(settings.never_touch, vec![PathBuf::from("/keep")]);
    }
}
