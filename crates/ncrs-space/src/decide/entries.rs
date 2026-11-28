use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime};

use ncrs_config::space::{Depth, SpaceSettings};
use ncrs_core::FolderSize;
use ncrs_fs::measure_folder_depth;

use super::named::named_entries;
use super::{
    sort_downloads_with, DecideKind, DownloadGroups, DownloadItem, InstalledApps, Matched,
    SystemInstalledApps,
};
use crate::facts::{DecideReason, EntryFacts};
use crate::scan::{Entry, EntryKind, Group};

/// A folder of your own counts as big from here on.
const BIG_FOLDER_BYTES: u64 = 500 * 1024 * 1024;
const AUTO_DEPTH: usize = 3;

/// The entries for the Decide group: sorted downloads, the known folders of your own
/// (session logs, backups, models, simulators) and big folders below home. Facts only;
/// nothing is judged beyond size, date and the group rules.
pub fn decide_entries(home: &Path, settings: &SpaceSettings, stop: &AtomicBool) -> Vec<Entry> {
    let downloads = home.join("Downloads");
    let mut entries = downloads_entries(
        &downloads,
        &SystemInstalledApps::new(home),
        &[home.join("Documents")],
        stop,
    );
    let named = named_entries(home, settings, stop);
    let named_paths: Vec<PathBuf> = named.iter().map(|entry| entry.path.clone()).collect();
    entries.extend(named);
    entries.extend(big_folder_entries(
        home,
        &downloads,
        &named_paths,
        settings,
        stop,
    ));
    entries.sort_by_key(|first| std::cmp::Reverse(first.size.bytes));
    entries
}

fn downloads_entries(
    folder: &Path,
    apps: &dyn InstalledApps,
    also_look_in: &[PathBuf],
    stop: &AtomicBool,
) -> Vec<Entry> {
    entries_of_groups(sort_downloads_with(
        folder,
        apps,
        stop,
        SystemTime::now(),
        also_look_in,
    ))
}

/// One entry per item of the groups, in the order of the groups.
pub fn entries_of_groups(groups: DownloadGroups) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut add = |item: &DownloadItem, kind: DecideKind, reason: DecideReason| {
        entries.push(entry_of(item, kind, reason));
    };
    for matched in &groups.installed {
        let reason = match &matched.installed {
            Some(app) => DecideReason::Installed(app.clone()),
            None => DecideReason::Other,
        };
        add(&matched.item, DecideKind::InstalledApp, reason);
    }
    for matched in &groups.unpacked {
        add(
            &matched.item,
            DecideKind::Unpacked,
            DecideReason::Unpacked {
                folder: matched.with.clone(),
            },
        );
    }
    for set in &groups.duplicates {
        for item in &set.extra {
            add(
                item,
                DecideKind::Duplicate,
                DecideReason::Duplicate {
                    copy: set.keep.path.clone(),
                    in_downloads: true,
                },
            );
        }
    }
    for Matched { item, with, .. } in &groups.elsewhere {
        add(
            item,
            DecideKind::Duplicate,
            DecideReason::Duplicate {
                copy: with.clone(),
                in_downloads: false,
            },
        );
    }
    for item in &groups.untouched_year {
        add(item, DecideKind::UntouchedYear, DecideReason::UntouchedYear);
    }
    for item in &groups.rest {
        add(item, DecideKind::Other, DecideReason::Other);
    }
    entries
}

fn entry_of(item: &DownloadItem, kind: DecideKind, reason: DecideReason) -> Entry {
    Entry {
        path: item.path.clone(),
        group: Group::Decide,
        kind: EntryKind::Decide(kind),
        size: FolderSize {
            bytes: item.size,
            partial: false,
        },
        modified: item.modified,
        device: device_of_path(&item.path),
        held_by: Vec::new(),
        facts: EntryFacts {
            last_used: item.last_opened.or(item.touched),
            last_opened: item.last_opened,
            open_count: item.open_count,
            reason: Some(reason),
            ..EntryFacts::default()
        },
    }
}

fn big_folder_entries(
    home: &Path,
    downloads: &Path,
    named: &[PathBuf],
    settings: &SpaceSettings,
    stop: &AtomicBool,
) -> Vec<Entry> {
    let depth = match settings.depth {
        Depth::Auto => AUTO_DEPTH,
        Depth::Levels(levels) => usize::from(levels).max(1),
    };
    let Some(tree) = measure_folder_depth(home, stop, depth, Duration::MAX, |_| {}) else {
        return Vec::new();
    };
    let wanted = |path: &Path, bytes: u64| {
        bytes >= BIG_FOLDER_BYTES
            && !path.starts_with(downloads)
            && !downloads.starts_with(path)
            && !named
                .iter()
                .any(|known| path.starts_with(known) || known.starts_with(path))
            && !is_left_alone(home, path)
            && !settings
                .never_touch
                .iter()
                .any(|never| path.starts_with(never))
    };
    let big: Vec<_> = tree
        .below
        .iter()
        .filter(|(path, size)| wanted(path, size.bytes))
        .collect();
    // The deepest big folders: a parent that holds a big folder is its sum, not a choice of its own.
    big.iter()
        .filter(|(path, _)| {
            !big.iter()
                .any(|(other, _)| other != path && other.starts_with(path))
        })
        .map(|(path, size)| {
            let modified = fs::metadata(path).and_then(|meta| meta.modified()).ok();
            Entry {
                path: path.clone(),
                group: Group::Decide,
                kind: EntryKind::Decide(DecideKind::BigFolder),
                size: *size,
                modified,
                device: device_of_path(path),
                held_by: Vec::new(),
                facts: EntryFacts {
                    last_used: modified,
                    ..EntryFacts::default()
                },
            }
        })
        .collect()
}

/// Hidden folders and the macOS library hold app data, not files of the user's own.
fn is_left_alone(home: &Path, path: &Path) -> bool {
    path.strip_prefix(home).is_ok_and(|relative| {
        relative.components().any(|part| {
            part.as_os_str()
                .to_str()
                .is_some_and(|name| name.starts_with('.') || name == "Library")
        })
    })
}

#[cfg(unix)]
pub fn device_of_path(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt as _;
    fs::symlink_metadata(path).ok().map(|meta| meta.dev())
}

#[cfg(not(unix))]
pub fn device_of_path(_path: &Path) -> Option<u64> {
    None
}
