//! The walk that finds repositories under the project folders and `~`.
//! It looks inside a repository for further ones, never follows a symlink, and
//! leaves out what the settings name, cloud folders and network drives.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use ncrs_config::repos::ReposSettings;

/// Names whose content a sync client downloads when it is read.
const CLOUD_NAMES: [&str; 6] = [
    "Dropbox",
    "CloudStorage",
    "Mobile Documents",
    "iCloud Drive",
    "Google Drive",
    "pCloud Drive",
];

/// Prefixes of the same kind (`OneDrive - Company`).
const CLOUD_PREFIXES: [&str; 2] = ["OneDrive", "Dropbox"];

/// File system types that live on another machine.
const NETWORK_TYPES: [&str; 6] = ["nfs", "nfs4", "cifs", "smb3", "smbfs", "fuse.sshfs"];

/// What a scan is told.
#[derive(Debug, Clone)]
pub struct ScanRequest {
    /// The folders the walk starts in, in this order.
    pub roots: Vec<PathBuf>,
    pub depth: u8,
    pub skip: Vec<String>,
    pub skip_cloud: bool,
    pub skip_network: bool,
    /// What a leading `~` in `skip` stands for.
    pub home: Option<PathBuf>,
    /// The App Store build: only below these folders. `None` everywhere else.
    pub allowed: Option<Vec<PathBuf>>,
    pub stop: Arc<AtomicBool>,
}

impl ScanRequest {
    /// The walk the settings ask for: the project folders, and `~` when that is on.
    pub fn from_settings(
        settings: &ReposSettings,
        home: Option<&Path>,
        allowed: Option<Vec<PathBuf>>,
        stop: Arc<AtomicBool>,
    ) -> Self {
        let mut roots = settings.sources.clone();
        if let (true, Some(home_dir)) = (settings.scan_home, home) {
            if !roots.iter().any(|root| root == home_dir) {
                roots.push(home_dir.to_path_buf());
            }
        }
        Self {
            roots,
            depth: settings.depth,
            skip: settings.skip.clone(),
            skip_cloud: settings.skip_cloud,
            skip_network: settings.skip_network,
            home: home.map(Path::to_path_buf),
            allowed,
            stop,
        }
    }
}

/// A repository and the folder the walk was started in to find it.
pub type Found = (PathBuf, PathBuf);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanResult {
    pub found: Vec<Found>,
    /// The roots that could be read.
    pub roots: Vec<PathBuf>,
    pub millis: u64,
    /// The stop flag ended the walk before it was done.
    pub stopped: bool,
}

/// Walks the roots; blocks.
pub fn scan(request: &ScanRequest) -> ScanResult {
    let started = Instant::now();
    let network = if request.skip_network {
        network_mounts()
    } else {
        Vec::new()
    };
    let walk = Walk { request, network };
    let mut found: Vec<Found> = Vec::new();
    let mut roots = Vec::new();
    for root in &request.roots {
        if walk.stopped() {
            break;
        }
        if !walk.may_enter(root, true) {
            continue;
        }
        let start = Place {
            dir: root,
            level: 0,
            inside_repo: false,
        };
        if root.read_dir().is_ok() {
            roots.push(root.clone());
        }
        walk.descend(root, start, &mut found);
    }
    // A repository below two roots belongs to the deeper one.
    found.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| right.1.cmp(&left.1)));
    found.dedup_by(|later, earlier| later.0 == earlier.0);
    ScanResult {
        found,
        roots,
        millis: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        stopped: walk.stopped(),
    }
}

/// Where the walk is.
#[derive(Clone, Copy)]
struct Place<'a> {
    dir: &'a Path,
    level: u8,
    inside_repo: bool,
}

struct Walk<'a> {
    request: &'a ScanRequest,
    network: Vec<PathBuf>,
}

impl Walk<'_> {
    fn stopped(&self) -> bool {
        self.request.stop.load(Ordering::Relaxed)
    }

    fn descend(&self, root: &Path, place: Place<'_>, found: &mut Vec<Found>) {
        if self.stopped() {
            return;
        }
        let git_entry = place.dir.join(".git").symlink_metadata().ok();
        // Inside a repository a `.git` file is one of its worktrees or submodules.
        if place.inside_repo && git_entry.as_ref().is_some_and(|entry| !entry.is_dir()) {
            return;
        }
        if git_entry.is_some() {
            found.push((place.dir.to_path_buf(), root.to_path_buf()));
        }
        if place.level >= self.request.depth {
            return;
        }
        let inside_repo = place.inside_repo || git_entry.is_some();
        let Ok(entries) = std::fs::read_dir(place.dir) else {
            return;
        };
        let mut folders: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter(|entry| entry.file_name() != ".git")
            .map(|entry| entry.path())
            .filter(|path| self.may_enter(path, false))
            .collect();
        folders.sort();
        for folder in folders {
            let below = Place {
                dir: &folder,
                level: place.level + 1,
                inside_repo,
            };
            self.descend(root, below, found);
        }
    }

    /// Whether the walk goes into `path`. A root the user named is only
    /// held to the sandbox; the names to skip are for what lies below.
    fn may_enter(&self, path: &Path, is_root: bool) -> bool {
        if let Some(allowed) = &self.request.allowed {
            if !allowed.iter().any(|grant| path.starts_with(grant)) {
                return false;
            }
        }
        if is_root {
            return true;
        }
        if self.is_skipped(path) {
            return false;
        }
        if self.request.skip_cloud && is_cloud_folder(path) {
            return false;
        }
        !(self.request.skip_network && self.is_network(path))
    }

    fn is_skipped(&self, path: &Path) -> bool {
        let name = path.file_name().and_then(|name| name.to_str());
        self.request.skip.iter().any(|entry| {
            if let Some(rest) = entry.strip_prefix('~') {
                let relative = rest.trim_start_matches(['/', '\\']);
                return self
                    .request
                    .home
                    .as_ref()
                    .is_some_and(|home| path == home.join(relative));
            }
            if entry.contains(['/', '\\']) {
                return path == Path::new(entry);
            }
            name == Some(entry.as_str())
        })
    }

    fn is_network(&self, path: &Path) -> bool {
        if cfg!(windows) && path.to_string_lossy().starts_with(r"\\") {
            return true;
        }
        if cfg!(target_os = "macos") && path.starts_with("/Volumes") {
            return true;
        }
        self.network.iter().any(|mount| path.starts_with(mount))
    }
}

fn is_cloud_folder(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    CLOUD_NAMES.contains(&name) || CLOUD_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

/// The mount points of file systems that live on another machine, from
/// `/proc/mounts`; empty where there is no such file.
fn network_mounts() -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string("/proc/mounts") else {
        return Vec::new();
    };
    network_mounts_in(&text)
}

fn network_mounts_in(mounts: &str) -> Vec<PathBuf> {
    mounts
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let (_device, point, kind) = (fields.next()?, fields.next()?, fields.next()?);
            NETWORK_TYPES
                .contains(&kind)
                .then(|| PathBuf::from(point.replace("\\040", " ")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(base: &Path, relative: &str) {
        std::fs::create_dir_all(base.join(relative).join(".git")).unwrap();
    }

    fn request(root: &Path, depth: u8) -> ScanRequest {
        ScanRequest {
            roots: vec![root.to_path_buf()],
            depth,
            skip: vec!["node_modules".to_owned()],
            skip_cloud: true,
            skip_network: false,
            home: None,
            allowed: None,
            stop: Arc::new(AtomicBool::new(false)),
        }
    }

    fn paths(result: &ScanResult, base: &Path) -> Vec<String> {
        result
            .found
            .iter()
            .map(|(path, _)| {
                path.strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }

    #[test]
    fn repos_are_found_down_to_the_depth() {
        let base = tempfile::tempdir().unwrap();
        repo(base.path(), "a");
        repo(base.path(), "group/b");
        repo(base.path(), "group/deeper/c");
        let shallow = scan(&request(base.path(), 2));
        assert_eq!(paths(&shallow, base.path()), ["a", "group/b"]);
        let deep = scan(&request(base.path(), 3));
        assert_eq!(
            paths(&deep, base.path()),
            ["a", "group/b", "group/deeper/c"]
        );
    }

    #[test]
    fn repos_inside_a_repo_are_found_down_to_the_depth() {
        let base = tempfile::tempdir().unwrap();
        repo(base.path(), "outer");
        repo(base.path(), "outer/inner");
        repo(base.path(), "outer/apps/shop");
        repo(base.path(), "outer/apps/deeper/far");
        repo(base.path(), "outer/node_modules/dep");
        repo(base.path(), "outer/.git/modules/lib");
        let result = scan(&request(base.path(), 3));
        assert_eq!(
            paths(&result, base.path()),
            ["outer", "outer/apps/shop", "outer/inner"]
        );
    }

    #[test]
    fn worktrees_and_submodules_inside_a_repo_are_not_listed_nor_entered() {
        let base = tempfile::tempdir().unwrap();
        repo(base.path(), "outer");
        repo(base.path(), "outer/.wt/feature/nested");
        std::fs::write(base.path().join("outer/.wt/feature/.git"), "gitdir: x").unwrap();
        let result = scan(&request(base.path(), 5));
        assert_eq!(paths(&result, base.path()), ["outer"]);
    }

    #[test]
    fn a_git_file_is_a_worktree_and_found_too() {
        let base = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(base.path().join("tree")).unwrap();
        std::fs::write(base.path().join("tree/.git"), "gitdir: /elsewhere").unwrap();
        let result = scan(&request(base.path(), 2));
        assert_eq!(paths(&result, base.path()), ["tree"]);
    }

    #[test]
    fn skipped_names_and_paths_stay_unread() {
        let base = tempfile::tempdir().unwrap();
        repo(base.path(), "node_modules/dep");
        repo(base.path(), "keep");
        repo(base.path(), "Library/inside");
        let mut asked = request(base.path(), 3);
        asked.home = Some(base.path().to_path_buf());
        asked.skip.push("~/Library".to_owned());
        let result = scan(&asked);
        assert_eq!(paths(&result, base.path()), ["keep"]);
    }

    #[test]
    fn cloud_folders_are_left_out_unless_asked_for() {
        let base = tempfile::tempdir().unwrap();
        repo(base.path(), "Dropbox/shared");
        repo(base.path(), "own");
        assert_eq!(paths(&scan(&request(base.path(), 3)), base.path()), ["own"]);
        let mut open = request(base.path(), 3);
        open.skip_cloud = false;
        assert_eq!(paths(&scan(&open), base.path()), ["Dropbox/shared", "own"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_not_followed() {
        let base = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        repo(elsewhere.path(), "linked");
        std::os::unix::fs::symlink(elsewhere.path(), base.path().join("link")).unwrap();
        repo(base.path(), "real");
        let result = scan(&request(base.path(), 3));
        assert_eq!(paths(&result, base.path()), ["real"]);
    }

    #[test]
    fn a_raised_stop_flag_ends_the_walk() {
        let base = tempfile::tempdir().unwrap();
        repo(base.path(), "a");
        let asked = request(base.path(), 3);
        asked.stop.store(true, Ordering::Relaxed);
        let result = scan(&asked);
        assert!(result.found.is_empty());
        assert!(result.stopped);
    }

    #[test]
    fn the_sandbox_walks_only_what_is_granted() {
        let base = tempfile::tempdir().unwrap();
        repo(base.path(), "granted/a");
        repo(base.path(), "other/b");
        let mut asked = request(base.path(), 3);
        asked.allowed = Some(vec![base.path().join("granted")]);
        assert!(
            scan(&asked).found.is_empty(),
            "the root itself is not granted"
        );
        asked.roots = vec![base.path().join("granted")];
        assert_eq!(paths(&scan(&asked), base.path()), ["granted/a"]);
    }

    #[test]
    fn network_mounts_come_from_the_proc_table() {
        let table = "/dev/sda1 / ext4 rw 0 0\nnas:/share /mnt/nas nfs4 rw 0 0\n//srv/x /mnt/my\\040disk cifs rw 0 0\n";
        assert_eq!(
            network_mounts_in(table),
            [PathBuf::from("/mnt/nas"), PathBuf::from("/mnt/my disk")]
        );
    }

    #[test]
    fn the_settings_make_the_roots() {
        let settings = ReposSettings {
            sources: vec![PathBuf::from("/work")],
            scan_home: true,
            ..ReposSettings::default()
        };
        let asked = ScanRequest::from_settings(
            &settings,
            Some(Path::new("/home/me")),
            None,
            Arc::new(AtomicBool::new(false)),
        );
        assert_eq!(
            asked.roots,
            [PathBuf::from("/work"), PathBuf::from("/home/me")]
        );
    }
}
