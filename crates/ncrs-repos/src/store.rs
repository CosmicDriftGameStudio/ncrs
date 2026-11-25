//! The list of repositories the user has, in `repos.toml`: where each came
//! from and whether it is a favourite. Kept apart from `config.toml`, which
//! would otherwise be rewritten every time a repo is learned.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// How a repository came into the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// A scan found it, in the folder the entry names as its source.
    Found,
    /// It was opened in a file panel.
    Learned,
    /// The user added the folder.
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoEntry {
    pub path: PathBuf,
    pub origin: Origin,
    /// The folder the scan that found it started in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<PathBuf>,
    #[serde(default)]
    pub favourite: bool,
}

/// What the last scan did, for the line of the settings page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanStamp {
    /// Seconds since 1970.
    pub at: u64,
    pub repos: u64,
    pub millis: u64,
}

impl ScanStamp {
    pub fn time(self) -> SystemTime {
        SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(self.at)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Store {
    /// The scan of `~` ran once; from then on only on request.
    #[serde(default)]
    pub home_scanned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_scan: Option<ScanStamp>,
    #[serde(default, rename = "repo")]
    pub repos: Vec<RepoEntry>,
}

impl Store {
    /// The file at `path`; a missing or unreadable one is an empty list.
    pub fn read(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| toml_edit::de::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn write(&self, path: &Path) -> Result<(), String> {
        let text = toml_edit::ser::to_string(self).map_err(|error| error.to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(path, text).map_err(|error| error.to_string())
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.repos.iter().any(|entry| entry.path == path)
    }

    pub fn entry(&self, path: &Path) -> Option<&RepoEntry> {
        self.repos.iter().find(|entry| entry.path == path)
    }

    pub fn is_favourite(&self, path: &Path) -> bool {
        self.entry(path).is_some_and(|entry| entry.favourite)
    }

    /// Adds a repository that is not in the list yet; says whether it did.
    pub fn add(&mut self, path: PathBuf, origin: Origin, source: Option<PathBuf>) -> bool {
        if self.contains(&path) {
            return false;
        }
        self.repos.push(RepoEntry {
            path,
            origin,
            source,
            favourite: false,
        });
        true
    }

    pub fn remove(&mut self, path: &Path) -> bool {
        let before = self.repos.len();
        self.repos.retain(|entry| entry.path != path);
        self.repos.len() != before
    }

    /// Flips the star; says the new state, `None` for a repo that is not listed.
    pub fn toggle_favourite(&mut self, path: &Path) -> Option<bool> {
        let entry = self.repos.iter_mut().find(|entry| entry.path == path)?;
        entry.favourite = !entry.favourite;
        Some(entry.favourite)
    }

    pub fn favourites(&self) -> Vec<PathBuf> {
        self.repos
            .iter()
            .filter(|entry| entry.favourite)
            .map(|entry| entry.path.clone())
            .collect()
    }

    /// The repos a scan found join the list; one that is listed already keeps
    /// its origin and star. Returns how many are new.
    pub fn merge_found(&mut self, found: &[(PathBuf, PathBuf)]) -> usize {
        found
            .iter()
            .filter(|(path, source)| self.add(path.clone(), Origin::Found, Some(source.clone())))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_is_written_and_read_back() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("repos.toml");
        let mut store = Store::default();
        store.add(
            PathBuf::from("/code/ncrs"),
            Origin::Found,
            Some(PathBuf::from("/code")),
        );
        store.add(PathBuf::from("/usb/notes"), Origin::Learned, None);
        store.add(PathBuf::from("/work/api"), Origin::Manual, None);
        store.toggle_favourite(Path::new("/code/ncrs"));
        store.home_scanned = true;
        store.last_scan = Some(ScanStamp {
            at: 1_700_000_000,
            repos: 3,
            millis: 2400,
        });
        store.write(&file).unwrap();
        assert_eq!(Store::read(&file), store);
    }

    #[test]
    fn a_missing_or_broken_file_is_an_empty_list() {
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(
            Store::read(&directory.path().join("none.toml")),
            Store::default()
        );
        let broken = directory.path().join("broken.toml");
        std::fs::write(&broken, "[[repo]]\npath = 3\n").unwrap();
        assert_eq!(Store::read(&broken), Store::default());
    }

    #[test]
    fn a_repo_is_listed_once() {
        let mut store = Store::default();
        assert!(store.add(PathBuf::from("/a"), Origin::Manual, None));
        assert!(!store.add(PathBuf::from("/a"), Origin::Learned, None));
        assert_eq!(store.repos.len(), 1);
    }

    #[test]
    fn a_scan_keeps_the_star_of_a_repo_it_finds_again() {
        let mut store = Store::default();
        store.add(PathBuf::from("/code/a"), Origin::Manual, None);
        store.toggle_favourite(Path::new("/code/a"));
        let new = store.merge_found(&[
            (PathBuf::from("/code/a"), PathBuf::from("/code")),
            (PathBuf::from("/code/b"), PathBuf::from("/code")),
        ]);
        assert_eq!(new, 1);
        assert!(store.is_favourite(Path::new("/code/a")));
        assert_eq!(
            store.entry(Path::new("/code/a")).unwrap().origin,
            Origin::Manual
        );
    }

    #[test]
    fn remove_and_the_star_of_a_stranger_do_nothing() {
        let mut store = Store::default();
        assert!(!store.remove(Path::new("/x")));
        assert_eq!(store.toggle_favourite(Path::new("/x")), None);
    }
}
