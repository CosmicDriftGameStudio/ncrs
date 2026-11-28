use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use ncrs_config::space::SpaceSettings;
use ncrs_core::FolderSize;
use ncrs_fs::{measure_folder_depth, RUNNING_REPORT_INTERVAL};
use ncrs_os_probe::{FolderUse, Holder, VolumeSpace};

use crate::catalog::{catalog_for, is_under_any, CatalogId, Os};
use crate::checks::{device_of, runs_tool, ProcessSource, FOLDER_USE_LIMIT};
use crate::decide::DecideKind;
use crate::drives::drive_name_of;
use crate::facts::{EntryFacts, UseCheck};
use crate::simulators::{device_bytes, Availability};
use crate::worktrees::{scan_worktrees, worktree_roots, WorktreeRoot, WorktreeSpace};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    Safe,
    Worktrees,
    Decide,
    InUse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    Catalog(CatalogId),
    Worktree(WorktreeSpace),
    Decide(DecideKind),
}

/// One measured folder. `size`, `modified` and `device` are what the check before the delete compares against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub group: Group,
    pub kind: EntryKind,
    pub size: FolderSize,
    pub modified: Option<SystemTime>,
    pub device: Option<u64>,
    pub held_by: Vec<Holder>,
    pub facts: EntryFacts,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scan {
    pub volume: VolumeSpace,
    /// The label of the measured drive; empty when it is not known.
    pub volume_name: String,
    /// The worktree folders with how many worktrees go and how many stay; empty when worktrees are off.
    pub worktree_roots: Vec<WorktreeRoot>,
    pub entries: Vec<Entry>,
    pub partial: bool,
    /// `false` when the running processes cannot be listed (App Store build): nothing
    /// is held back as in use, and the delete asks the user to confirm that.
    pub processes_known: bool,
    pub measured_at: SystemTime,
    pub took: Duration,
}

#[derive(Debug, Clone)]
pub struct ScanRequest {
    pub os: Os,
    pub home: PathBuf,
    pub repos: Vec<PathBuf>,
    pub settings: SpaceSettings,
}

/// Blocking; run on a worker thread. `None` when `stop` was set.
///
/// An entry whose use cannot be known (a folder that could not be checked) is listed as
/// in use without a holder. Without a process list no catalog entry is moved to in use;
/// the delete then needs the user's confirm (`UnknownUse::AllowAfterConfirm`).
pub fn scan(
    request: &ScanRequest,
    probes: &dyn ProcessSource,
    stop: &AtomicBool,
    mut on_running: impl FnMut(u64),
) -> Option<Scan> {
    let started = Instant::now();
    let settings = &request.settings;
    let volume = ncrs_os_probe::volume_space(&request.home).ok();
    let mut partial = volume.is_none();
    let home_device = std::fs::symlink_metadata(&request.home)
        .ok()
        .and_then(|metadata| device_of(&metadata));
    let processes = probes.processes();
    let processes_known = processes.is_some();
    let env = |name: &str| std::env::var_os(name);
    let worktrees = if settings.show_worktrees {
        scan_worktrees(&request.repos, stop)
    } else {
        crate::worktrees::WorktreeScan::default()
    };
    if stop.load(Ordering::Relaxed) {
        return None;
    }
    let merged_paths: Vec<PathBuf> = worktrees
        .merged
        .iter()
        .map(|worktree| worktree.path.clone())
        .collect();
    let places = catalog_for(
        request.os,
        &request.home,
        &env,
        &request.repos,
        settings,
        &merged_paths,
    );

    let mut entries: Vec<Entry> = Vec::new();
    let mut counted: u64 = 0;
    for place in places {
        if stop.load(Ordering::Relaxed) {
            return None;
        }
        let Some(measured) = measure(&place.path, place.id, stop, counted, &mut on_running)? else {
            continue;
        };
        counted = counted.saturating_add(measured.size.bytes);
        if measured.size.bytes == 0 {
            continue;
        }
        let (group, held_by, use_check) = if processes.is_some() {
            use_of(
                &place.path,
                place.tools,
                processes.as_deref(),
                probes,
                Group::Safe,
            )
        } else {
            (Group::Safe, Vec::new(), UseCheck::NotChecked)
        };
        entries.push(Entry {
            path: place.path,
            group,
            kind: EntryKind::Catalog(place.id),
            size: measured.size,
            modified: measured.modified,
            device: measured.device,
            held_by,
            facts: EntryFacts {
                use_check,
                last_used: measured.modified,
                ..EntryFacts::default()
            },
        });
    }

    let mut worktrees_in_use: HashSet<PathBuf> = HashSet::new();
    for worktree in &worktrees.merged {
        if stop.load(Ordering::Relaxed) {
            return None;
        }
        let device = std::fs::symlink_metadata(&worktree.path)
            .ok()
            .and_then(|metadata| device_of(&metadata));
        let (group, held_by, use_check) = use_of(
            &worktree.path,
            &[],
            processes.as_deref(),
            probes,
            Group::Worktrees,
        );
        if group == Group::InUse {
            worktrees_in_use.insert(worktree.path.clone());
        }
        entries.push(Entry {
            path: worktree.path.clone(),
            group,
            size: worktree.size,
            modified: worktree.modified,
            device,
            held_by,
            facts: EntryFacts {
                use_check,
                last_used: worktree.modified,
                ..EntryFacts::default()
            },
            kind: EntryKind::Worktree(worktree.clone()),
        });
    }
    let worktree_roots = worktree_roots(&worktrees.merged, &worktrees_in_use, &worktrees.kept);

    entries.extend(crate::decide::decide_entries(&request.home, settings, stop));
    if stop.load(Ordering::Relaxed) {
        return None;
    }

    entries.retain(|entry| !is_under_any(&entry.path, &settings.never_touch));
    if !settings.other_drives && home_device.is_some() {
        entries.retain(|entry| entry.device.is_none() || entry.device == home_device);
    }
    partial |= entries.iter().any(|entry| entry.size.partial);
    entries.sort_by_key(|left| std::cmp::Reverse(left.size.bytes));

    Some(Scan {
        volume: volume.unwrap_or(VolumeSpace {
            total: 0,
            available: 0,
        }),
        volume_name: drive_name_of(&request.home, &request.home).unwrap_or_default(),
        worktree_roots,
        entries,
        partial,
        processes_known,
        measured_at: SystemTime::now(),
        took: started.elapsed(),
    })
}

struct Measured {
    size: FolderSize,
    modified: Option<SystemTime>,
    device: Option<u64>,
}

/// `None` when stopped; `Some(None)` when the path is gone or is a symlink.
fn measure(
    path: &Path,
    id: CatalogId,
    stop: &AtomicBool,
    counted: u64,
    on_running: &mut impl FnMut(u64),
) -> Option<Option<Measured>> {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return Some(None);
    };
    if metadata.file_type().is_symlink() {
        return Some(None);
    }
    let size = if id == CatalogId::UnavailableSimulators {
        FolderSize {
            bytes: device_bytes(path, Availability::Unavailable, stop)?,
            partial: false,
        }
    } else if metadata.is_dir() {
        let tree = measure_folder_depth(path, stop, 0, RUNNING_REPORT_INTERVAL, |bytes| {
            on_running(counted.saturating_add(bytes));
        })?;
        tree.total
    } else {
        FolderSize {
            bytes: metadata.len(),
            partial: false,
        }
    };
    Some(Some(Measured {
        size,
        modified: metadata.modified().ok(),
        device: device_of(&metadata),
    }))
}

/// The group the entry is in, who holds it, and the answer of the check. A running tool or an
/// open file moves it to `InUse`; an answer that cannot be had does too, with no holder.
fn use_of(
    path: &Path,
    tools: &[&str],
    processes: Option<&[Holder]>,
    probes: &dyn ProcessSource,
    group: Group,
) -> (Group, Vec<Holder>, UseCheck) {
    let Some(running) = processes else {
        return (Group::InUse, Vec::new(), UseCheck::NotChecked);
    };
    let tool_holders: Vec<Holder> = running
        .iter()
        .filter(|holder| runs_tool(holder, tools))
        .cloned()
        .collect();
    if !tool_holders.is_empty() {
        return (
            Group::InUse,
            tool_holders.clone(),
            UseCheck::Held(tool_holders),
        );
    }
    match probes.folder_use(path, FOLDER_USE_LIMIT) {
        FolderUse::Free => (group, Vec::new(), UseCheck::Free),
        FolderUse::Held(holders) => (Group::InUse, holders.clone(), UseCheck::Held(holders)),
        FolderUse::Unknown(why) => (Group::InUse, Vec::new(), UseCheck::Unknown(why)),
    }
}
