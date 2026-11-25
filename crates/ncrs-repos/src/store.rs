//! The list of repositories the user has, in `repos.toml`: where each came
//! from and whether it is a favourite. Kept apart from `config.toml`, which
//! would otherwise be rewritten every time a repo is learned.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ncrs_config::config::replace_file_atomically;
use serde::{Deserialize, Serialize};

use crate::scan::ScanResult;

/// The newest shape of the file this build reads and the one it writes.
const CURRENT_VERSION: u32 = 1;

const fn first_version() -> u32 {
    1
}

/// Why the list is not read or not written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// The file is there but is not a list this build understands.
    Unreadable(String),
    /// A newer ncrs wrote the file.
    Newer,
    NotWritten(String),
}

/// Read before the rest: a newer file may not have today's shape.
#[derive(Deserialize)]
struct VersionOnly {
    #[serde(default = "first_version")]
    version: u32,
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Store {
    #[serde(default = "first_version")]
    pub version: u32,
    /// The scan of `~` ran once; from then on only on request.
    #[serde(default)]
    pub home_scanned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_scan: Option<ScanStamp>,
    #[serde(default, rename = "repo")]
    pub repos: Vec<RepoEntry>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            home_scanned: false,
            last_scan: None,
            repos: Vec::new(),
        }
    }
}

impl Store {
    /// The file at `path`; a missing one is an empty list.
    pub fn read(path: &Path) -> Result<Self, StoreError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(StoreError::Unreadable(error.to_string())),
        };
        let unreadable =
            |error: toml_edit::de::Error| StoreError::Unreadable(error.message().to_owned());
        let found: VersionOnly = toml_edit::de::from_str(&text).map_err(unreadable)?;
        if found.version > CURRENT_VERSION {
            return Err(StoreError::Newer);
        }
        toml_edit::de::from_str(&text).map_err(unreadable)
    }

    /// Replaces the file at `path` in one step. One that cannot be read is left
    /// alone: it holds a list this build would destroy.
    pub fn write(&self, path: &Path) -> Result<(), StoreError> {
        Self::read(path)?;
        let not_written =
            |reason: &dyn std::fmt::Display| StoreError::NotWritten(reason.to_string());
        let text = toml_edit::ser::to_string(self).map_err(|error| not_written(&error))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| not_written(&error))?;
        }
        replace_file_atomically(path, ".repos.toml.", &text)
            .map_err(|(_, error)| not_written(&error))
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
        let before = self.repos.len();
        for (path, source) in found {
            match self.repos.iter_mut().find(|entry| entry.path == *path) {
                Some(entry) if entry.origin == Origin::Found => {
                    entry.source = Some(source.clone());
                }
                Some(_) => {}
                None => {
                    self.add(path.clone(), Origin::Found, Some(source.clone()));
                }
            }
        }
        self.repos.len() - before
    }

    /// Drops what earlier scans found and `scan` did not, below a folder it read
    /// or below one that is not among the `sources` any more. Favourites stay,
    /// and folders that are gone: their drive may be unplugged. Returns the dropped.
    pub fn drop_unfound(&mut self, scan: &ScanResult, sources: &[PathBuf]) -> Vec<PathBuf> {
        let is_stale = |entry: &RepoEntry| {
            entry.origin == Origin::Found
                && !entry.favourite
                && entry
                    .source
                    .as_ref()
                    .is_some_and(|source| scan.roots.contains(source) || !sources.contains(source))
                && !scan.found.iter().any(|(path, _)| *path == entry.path)
                && entry.path.is_dir()
        };
        let (dropped, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.repos)
            .into_iter()
            .partition(is_stale);
        self.repos = kept;
        dropped.into_iter().map(|entry| entry.path).collect()
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
        assert_eq!(Store::read(&file), Ok(store));
        assert_eq!(
            std::fs::read_dir(directory.path()).unwrap().count(),
            1,
            "no temporary file stays behind"
        );
    }

    #[test]
    fn a_missing_file_is_an_empty_list() {
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(
            Store::read(&directory.path().join("none.toml")),
            Ok(Store::default())
        );
    }

    #[test]
    fn a_broken_or_newer_file_is_never_overwritten() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("repos.toml");
        for text in [
            "[[repo]]\npath = 3\n",
            "[[repo",
            "version = 2\n\n[[repository]]\npath = \"/a\"\n",
        ] {
            std::fs::write(&file, text).unwrap();
            assert!(Store::read(&file).is_err(), "{text}");
            assert!(Store::default().write(&file).is_err(), "{text}");
            assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
        }
        assert_eq!(Store::read(&file), Err(StoreError::Newer));
    }

    #[test]
    fn a_file_without_a_version_is_the_first_one() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("repos.toml");
        std::fs::write(&file, "home_scanned = true\n").unwrap();
        assert!(Store::read(&file).unwrap().home_scanned);
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

    #[test]
    fn a_scan_drops_what_it_no_longer_finds_where_it_looked() {
        let base = tempfile::tempdir().unwrap();
        let folder = |name: &str| {
            let path = base.path().join(name);
            std::fs::create_dir_all(&path).unwrap();
            path
        };
        let (code, old_source, unread) = (folder("code"), folder("old"), folder("unread"));
        let mut store = Store::default();
        let mut add = |name: &str, origin: Origin, source: &Path| {
            let path = folder(name);
            store.add(path.clone(), origin, Some(source.to_path_buf()));
            path
        };
        add("code/stale", Origin::Found, &code);
        add("old/of-a-removed-source", Origin::Found, &old_source);
        let kept = [
            add("code/found", Origin::Found, &code),
            add("code/favourite", Origin::Found, &code),
            add("code/learned", Origin::Learned, &code),
            add("unread/found-before", Origin::Found, &unread),
        ];
        let unplugged = code.join("unplugged");
        store.add(unplugged.clone(), Origin::Found, Some(code.clone()));
        store.toggle_favourite(&kept[1]);
        let scan = ScanResult {
            found: vec![(kept[0].clone(), code.clone())],
            roots: vec![code.clone()],
            ..ScanResult::default()
        };
        let dropped = store.drop_unfound(&scan, &[code, unread]);
        assert_eq!(dropped.len(), 2);
        let left: Vec<&PathBuf> = store.repos.iter().map(|entry| &entry.path).collect();
        assert_eq!(left.len(), 5);
        assert!(kept.iter().chain([&unplugged]).all(|path| left.contains(&path)));
    }
}
