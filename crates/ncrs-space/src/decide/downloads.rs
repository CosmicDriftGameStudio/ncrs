use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use ncrs_fs::measure_folder;
use ncrs_intake::hash_path;

use super::apps::{app_name_of_installer, compare_versions, version_of_installer, InstalledApps};
use crate::catalog::touches_protected;
use crate::facts::InstalledApp;

const INSTALLER_EXTENSIONS: [&str; 6] = ["dmg", "pkg", "xip", "exe", "msi", "appimage"];
const ARCHIVE_SUFFIXES: [&str; 9] = [
    ".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst", ".tgz", ".zip", ".7z", ".rar", ".tar",
];
/// Names of downloads that are not finished yet.
const PARTIAL_EXTENSIONS: [&str; 4] = ["crdownload", "part", "download", "partial"];
/// A copy elsewhere is only looked for from this size on, and in at most this many files.
const ELSEWHERE_MIN_BYTES: u64 = 4096;
const ELSEWHERE_MAX_FILES: usize = 200_000;
const ELSEWHERE_MAX_DEPTH: usize = 8;
/// Only this many items ask the system when they were opened; each question is one process.
const USAGE_ITEMS_MAX: usize = 300;
const YEAR: Duration = Duration::from_secs(365 * 24 * 60 * 60);

/// One thing in the Downloads folder, as it was when it was looked at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadItem {
    pub path: PathBuf,
    pub size: u64,
    pub modified: Option<SystemTime>,
    /// The last time it was opened or changed, whichever is later.
    pub touched: Option<SystemTime>,
    pub is_folder: bool,
    /// When the system says it was opened last (macOS Spotlight); `None` where it does not know.
    pub last_opened: Option<SystemTime>,
    pub open_count: Option<u32>,
}

/// An item and the thing that makes it redundant: the installed app for an
/// installer, the unpacked folder for an archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matched {
    pub item: DownloadItem,
    pub with: PathBuf,
    /// For an installer: the app, its version and how it compares. `None` for the others.
    pub installed: Option<InstalledApp>,
}

/// Files with the same content. `keep` is the oldest; only `extra` is offered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateSet {
    pub keep: DownloadItem,
    pub extra: Vec<DownloadItem>,
}

/// Every item of the folder lands in exactly one group, the first that fits
/// in this order. The kept file of a duplicate set is not in `duplicates`;
/// it goes on to the later groups.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DownloadGroups {
    pub installed: Vec<Matched>,
    pub unpacked: Vec<Matched>,
    pub duplicates: Vec<DuplicateSet>,
    /// Items whose content also lies outside Downloads; `with` is that copy.
    pub elsewhere: Vec<Matched>,
    pub untouched_year: Vec<DownloadItem>,
    pub rest: Vec<DownloadItem>,
}

impl DownloadGroups {
    /// The groups without the items that touch a protected folder (see
    /// [`touches_protected`]): they are neither offered nor picked.
    #[must_use]
    pub fn without_protected(mut self, protected: &[PathBuf]) -> Self {
        let free = |item: &DownloadItem| !touches_protected(&item.path, protected);
        self.installed.retain(|matched| free(&matched.item));
        self.unpacked.retain(|matched| free(&matched.item));
        self.elsewhere.retain(|matched| free(&matched.item));
        for set in &mut self.duplicates {
            set.extra.retain(free);
        }
        self.duplicates.retain(|set| !set.extra.is_empty());
        self.untouched_year.retain(free);
        self.rest.retain(free);
        self
    }
}

/// Sorts what lies directly in `folder`. When `stop` is set the groups found so
/// far come back; hidden entries, links and unfinished downloads are left out.
pub fn sort_downloads(
    folder: &Path,
    apps: &dyn InstalledApps,
    stop: &AtomicBool,
) -> DownloadGroups {
    sort_downloads_at(folder, apps, stop, SystemTime::now())
}

/// [`sort_downloads`] with the clock given, for tests.
pub fn sort_downloads_at(
    folder: &Path,
    apps: &dyn InstalledApps,
    stop: &AtomicBool,
    now: SystemTime,
) -> DownloadGroups {
    sort_downloads_with(folder, apps, stop, now, &[])
}

/// [`sort_downloads_at`] that also looks for copies of the files below `also_look_in`
/// (for example `~/Documents`), so a copy kept outside Downloads makes the download redundant.
pub fn sort_downloads_with(
    folder: &Path,
    apps: &dyn InstalledApps,
    stop: &AtomicBool,
    now: SystemTime,
    also_look_in: &[PathBuf],
) -> DownloadGroups {
    let mut groups = DownloadGroups::default();
    let mut items = list_items(folder, stop);
    let folders: HashMap<String, PathBuf> = items
        .iter()
        .filter(|item| item.is_folder)
        .filter_map(|item| Some((name_of(&item.path)?.to_lowercase(), item.path.clone())))
        .collect();

    items.retain(|item| {
        if let Some(app) = installed_app_of(item, apps) {
            groups.installed.push(Matched {
                item: item.clone(),
                with: app.path.clone(),
                installed: Some(app),
            });
            return false;
        }
        if let Some(unpacked) = unpacked_folder_of(item, &folders) {
            groups.unpacked.push(Matched {
                item: item.clone(),
                with: unpacked,
                installed: None,
            });
            return false;
        }
        true
    });

    groups.duplicates = duplicate_sets(&items, stop);
    let extras: HashSet<&Path> = groups
        .duplicates
        .iter()
        .flat_map(|set| set.extra.iter().map(|item| item.path.as_path()))
        .collect();
    let remaining: Vec<DownloadItem> = items
        .iter()
        .filter(|item| !extras.contains(item.path.as_path()))
        .cloned()
        .collect();
    groups.elsewhere = copies_elsewhere(&remaining, also_look_in, stop);
    let copied: HashSet<&Path> = groups
        .elsewhere
        .iter()
        .map(|matched| matched.item.path.as_path())
        .collect();
    let unmatched: Vec<DownloadItem> = remaining
        .iter()
        .filter(|item| !copied.contains(item.path.as_path()))
        .cloned()
        .collect();

    for item in unmatched {
        let untouched = item
            .touched
            .is_some_and(|touched| now.duration_since(touched).is_ok_and(|age| age >= YEAR));
        if untouched {
            groups.untouched_year.push(item);
        } else {
            groups.rest.push(item);
        }
    }
    groups
}

fn name_of(path: &Path) -> Option<String> {
    path.file_name()?.to_str().map(str::to_owned)
}

fn list_items(folder: &Path, stop: &AtomicBool) -> Vec<DownloadItem> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for entry in entries.flatten() {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let path = entry.path();
        let Some(name) = name_of(&path) else {
            continue;
        };
        let is_partial = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                PARTIAL_EXTENSIONS.contains(&extension.to_lowercase().as_str())
            });
        if name.starts_with('.') || is_partial {
            continue;
        }
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        let size = if meta.is_dir() {
            match measure_folder(&path, stop) {
                Some(size) => size.bytes,
                None => break,
            }
        } else {
            meta.len()
        };
        let modified = meta.modified().ok();
        let usage = if items.len() < USAGE_ITEMS_MAX {
            ncrs_os_probe::file_usage(&path)
        } else {
            ncrs_os_probe::FileUsage::default()
        };
        let touched = [modified, meta.accessed().ok(), usage.last_opened]
            .into_iter()
            .flatten()
            .max();
        items.push(DownloadItem {
            path,
            size,
            modified,
            touched,
            is_folder: meta.is_dir(),
            last_opened: usage.last_opened,
            open_count: usage.open_count,
        });
    }
    items.sort_by(|first, second| first.path.cmp(&second.path));
    items
}

fn installed_app_of(item: &DownloadItem, apps: &dyn InstalledApps) -> Option<InstalledApp> {
    if item.is_folder {
        return None;
    }
    let extension = item.path.extension()?.to_str()?.to_lowercase();
    if !INSTALLER_EXTENSIONS.contains(&extension.as_str()) {
        return None;
    }
    let file_name = name_of(&item.path)?;
    let path = apps.find(&app_name_of_installer(&file_name)?)?;
    let version = apps.version(&path);
    let installer_version = version_of_installer(&file_name);
    Some(InstalledApp {
        relation: compare_versions(version.as_deref(), installer_version.as_deref()),
        path,
        version,
        installer_version,
    })
}

/// Files below `roots` with the same content as an item. Hidden folders, links and
/// `node_modules` are not entered; the walk ends after a fixed number of files.
fn copies_elsewhere(items: &[DownloadItem], roots: &[PathBuf], stop: &AtomicBool) -> Vec<Matched> {
    let wanted: HashSet<u64> = items
        .iter()
        .filter(|item| !item.is_folder && item.size >= ELSEWHERE_MIN_BYTES)
        .map(|item| item.size)
        .collect();
    if wanted.is_empty() || roots.is_empty() {
        return Vec::new();
    }
    let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    let mut seen = 0;
    for root in roots {
        walk_for_sizes(root, &wanted, &mut by_size, &mut seen, 0, stop);
    }
    let mut matched = Vec::new();
    for item in items.iter().filter(|item| !item.is_folder) {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let Some(candidates) = by_size.get_mut(&item.size) else {
            continue;
        };
        candidates.sort();
        let Ok(hash) = hash_path(&item.path) else {
            continue;
        };
        let same_content = candidates
            .iter()
            .find(|candidate| hash_path(candidate).is_ok_and(|other| other == hash));
        if let Some(copy) = same_content {
            matched.push(Matched {
                item: item.clone(),
                with: copy.clone(),
                installed: None,
            });
        }
    }
    matched
}

fn walk_for_sizes(
    folder: &Path,
    wanted: &HashSet<u64>,
    found: &mut HashMap<u64, Vec<PathBuf>>,
    seen: &mut usize,
    depth: usize,
    stop: &AtomicBool,
) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        if stop.load(Ordering::Relaxed) || *seen >= ELSEWHERE_MAX_FILES {
            return;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        let hidden =
            name_of(&path).is_none_or(|name| name.starts_with('.') || name == "node_modules");
        if kind.is_symlink() || hidden {
            continue;
        }
        if kind.is_dir() {
            if depth < ELSEWHERE_MAX_DEPTH {
                walk_for_sizes(&path, wanted, found, seen, depth + 1, stop);
            }
        } else if let Ok(meta) = entry.metadata() {
            if meta.is_file() {
                *seen += 1;
                if wanted.contains(&meta.len()) {
                    found.entry(meta.len()).or_default().push(path);
                }
            }
        }
    }
}

fn unpacked_folder_of(item: &DownloadItem, folders: &HashMap<String, PathBuf>) -> Option<PathBuf> {
    if item.is_folder {
        return None;
    }
    let name = name_of(&item.path)?;
    let lower = name.to_lowercase();
    let suffix = ARCHIVE_SUFFIXES
        .iter()
        .find(|suffix| lower.ends_with(**suffix))?;
    folders
        .get(lower.get(..lower.len() - suffix.len())?)
        .cloned()
}

fn duplicate_sets(items: &[DownloadItem], stop: &AtomicBool) -> Vec<DuplicateSet> {
    let mut by_size: HashMap<u64, Vec<&DownloadItem>> = HashMap::new();
    for item in items.iter().filter(|item| !item.is_folder && item.size > 0) {
        by_size.entry(item.size).or_default().push(item);
    }
    let mut sets = Vec::new();
    for same_size in by_size.into_values().filter(|group| group.len() > 1) {
        let mut by_hash: HashMap<_, Vec<&DownloadItem>> = HashMap::new();
        for item in same_size {
            if stop.load(Ordering::Relaxed) {
                return sets;
            }
            if let Ok(hash) = hash_path(&item.path) {
                by_hash.entry(hash).or_default().push(item);
            }
        }
        for mut same in by_hash.into_values().filter(|group| group.len() > 1) {
            same.sort_by(|first, second| {
                first
                    .modified
                    .cmp(&second.modified)
                    .then_with(|| first.path.cmp(&second.path))
            });
            let mut copies = same.into_iter().cloned();
            if let Some(keep) = copies.next() {
                sets.push(DuplicateSet {
                    keep,
                    extra: copies.collect(),
                });
            }
        }
    }
    sets.sort_by(|first, second| first.keep.path.cmp(&second.keep.path));
    sets
}
