//! The `[repos]` section: where the repo manager looks for repositories and
//! how the bulk actions run. The list of repositories itself is not here, it
//! changes with every repo the user opens and lives in `repos.toml`.

use std::path::PathBuf;

use serde::Deserialize;

/// Levels under each source the search goes down, when nothing says otherwise.
pub const DEFAULT_DEPTH: u8 = 3;
/// The depths the settings page offers; the file may say any of 1 to [`MAX_DEPTH`].
pub const DEPTH_CHOICES: [u8; 4] = [2, 3, 4, 5];
pub const MAX_DEPTH: u8 = 8;
/// Repos a bulk action works on at the same time, when nothing says otherwise.
pub const DEFAULT_CONCURRENCY: u8 = 4;
/// The numbers the settings page offers; the file may say any of 1 to [`MAX_CONCURRENCY`].
pub const CONCURRENCY_CHOICES: [u8; 3] = [2, 4, 8];
pub const MAX_CONCURRENCY: u8 = 16;

/// The `[repos]` section as written, unchecked.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReposSection {
    #[serde(default)]
    pub sources: Option<Vec<PathBuf>>,
    #[serde(default)]
    pub scan_home: Option<bool>,
    /// Checked by `resolve_repos`: outside 1 to [`MAX_DEPTH`] is the default.
    #[serde(default)]
    pub depth: Option<i64>,
    /// Folder names, or paths that start with `~`, the search does not enter.
    #[serde(default)]
    pub skip: Option<Vec<String>>,
    #[serde(default)]
    pub skip_cloud: Option<bool>,
    #[serde(default)]
    pub skip_network: Option<bool>,
    #[serde(default)]
    pub learn_from_panel: Option<bool>,
    /// Checked by `resolve_repos`: outside 1 to [`MAX_CONCURRENCY`] is the default.
    #[serde(default)]
    pub concurrency: Option<i64>,
}

/// What the repo manager reads from `[repos]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReposSettings {
    /// The project folders the search walks, besides `~` when `scan_home` is on.
    pub sources: Vec<PathBuf>,
    pub scan_home: bool,
    pub depth: u8,
    pub skip: Vec<String>,
    /// iCloud, Dropbox, OneDrive and the like stay unread: reading would download them.
    pub skip_cloud: bool,
    pub skip_network: bool,
    /// Every repo opened in a file panel joins the list.
    pub learn_from_panel: bool,
    pub concurrency: u8,
}

impl ReposSettings {
    /// Whether a search with `other` would find the same.
    pub fn searches_like(&self, other: &Self) -> bool {
        let same_search = Self {
            learn_from_panel: self.learn_from_panel,
            concurrency: self.concurrency,
            ..other.clone()
        };
        *self == same_search
    }
}

/// What the search leaves out unless the file lists its own.
pub fn default_skip() -> Vec<String> {
    let mut skip = vec!["node_modules".to_owned(), "target".to_owned()];
    if cfg!(target_os = "macos") {
        skip.push("~/Library".to_owned());
    }
    if cfg!(windows) {
        skip.push("AppData".to_owned());
    }
    skip
}

impl Default for ReposSettings {
    fn default() -> Self {
        Self {
            sources: Vec::new(),
            scan_home: false,
            depth: DEFAULT_DEPTH,
            skip: default_skip(),
            skip_cloud: true,
            skip_network: true,
            learn_from_panel: true,
            concurrency: DEFAULT_CONCURRENCY,
        }
    }
}

impl ReposSettings {
    /// The search has somewhere to look.
    pub fn has_roots(&self) -> bool {
        self.scan_home || !self.sources.is_empty()
    }
}

/// Checks `section`: a number outside its range is the default, an empty entry of `skip` is dropped.
pub fn resolve_repos(section: &ReposSection) -> ReposSettings {
    let defaults = ReposSettings::default();
    let within = |value: Option<i64>, max: u8, fallback: u8| {
        value
            .and_then(|number| u8::try_from(number).ok())
            .filter(|number| (1..=max).contains(number))
            .unwrap_or(fallback)
    };
    ReposSettings {
        sources: section.sources.clone().unwrap_or(defaults.sources),
        scan_home: section.scan_home.unwrap_or(defaults.scan_home),
        depth: within(section.depth, MAX_DEPTH, defaults.depth),
        skip: section.skip.as_ref().map_or(defaults.skip, |listed| {
            listed
                .iter()
                .map(|entry| entry.trim().to_owned())
                .filter(|entry| !entry.is_empty())
                .collect()
        }),
        skip_cloud: section.skip_cloud.unwrap_or(defaults.skip_cloud),
        skip_network: section.skip_network.unwrap_or(defaults.skip_network),
        learn_from_panel: section
            .learn_from_panel
            .unwrap_or(defaults.learn_from_panel),
        concurrency: within(section.concurrency, MAX_CONCURRENCY, defaults.concurrency),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_written_is_the_defaults() {
        assert_eq!(
            resolve_repos(&ReposSection::default()),
            ReposSettings::default()
        );
    }

    #[test]
    fn a_number_out_of_range_is_the_default() {
        let section = ReposSection {
            depth: Some(0),
            concurrency: Some(99),
            ..ReposSection::default()
        };
        let settings = resolve_repos(&section);
        assert_eq!(settings.depth, DEFAULT_DEPTH);
        assert_eq!(settings.concurrency, DEFAULT_CONCURRENCY);
    }

    #[test]
    fn an_own_skip_list_replaces_the_defaults_and_loses_empty_entries() {
        let section = ReposSection {
            skip: Some(vec![" vendor ".to_owned(), String::new()]),
            ..ReposSection::default()
        };
        assert_eq!(resolve_repos(&section).skip, vec!["vendor".to_owned()]);
    }
}
