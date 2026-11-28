//! The folders of your own that are known by place: session logs, backups, models and
//! the simulators in use. Hidden or under the library, so the search for big folders
//! skips them; they are listed by exact path and always offered for a decision.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use ncrs_config::space::SpaceSettings;
use ncrs_fs::measure_folder;

use super::entries::device_of_path;
use super::DecideKind;
use crate::catalog::touches_protected;
use crate::facts::EntryFacts;
use crate::scan::{Entry, EntryKind, Group};
use crate::simulators::{device_bytes, Availability};

const SESSION_LOG_FOLDERS: [&str; 1] = [".codex/sessions"];
/// Folders of the Claude tools: `.claude`, `.claude-work`, … each with a `projects` folder.
const CLAUDE_PREFIX: &str = ".claude";
const BACKUP_FOLDERS: [&str; 4] = [
    "Backups",
    "Backup",
    "Library/Application Support/MobileSync/Backup",
    "AppData/Roaming/Apple Computer/MobileSync/Backup",
];
const MODEL_FOLDERS: [&str; 7] = [
    ".cache/huggingface",
    ".cache/torch",
    ".cache/whisper",
    ".cache/lm-studio",
    ".ollama/models",
    ".lmstudio/models",
    "Library/Caches/huggingface",
];
/// Folders under the cache roots that are named like models are taken as downloaded models.
const MODEL_WORD: &str = "model";
const CORE_SIMULATOR: &str = "Library/Developer/CoreSimulator";

pub fn named_entries(home: &Path, settings: &SpaceSettings, stop: &AtomicBool) -> Vec<Entry> {
    let mut places: Vec<(PathBuf, DecideKind)> = Vec::new();
    for path in session_log_folders(home) {
        places.push((path, DecideKind::SessionLogs));
    }
    for relative in BACKUP_FOLDERS {
        places.push((home.join(relative), DecideKind::Backups));
    }
    for path in model_folders(home) {
        places.push((path, DecideKind::ModelCache));
    }

    let mut entries: Vec<Entry> = Vec::new();
    let mut taken: Vec<PathBuf> = Vec::new();
    for (path, kind) in places {
        let is_real_folder =
            fs::symlink_metadata(&path).is_ok_and(|found| found.file_type().is_dir());
        let overlaps = taken
            .iter()
            .any(|known| path.starts_with(known) || known.starts_with(&path));
        let never = touches_protected(&path, &settings.never_touch);
        if !is_real_folder || overlaps || never {
            continue;
        }
        let measured = if kind == DecideKind::SessionLogs {
            session_log_size(&path, stop)
        } else {
            measure_folder(&path, stop)
        };
        let Some(size) = measured else {
            return entries;
        };
        if size.bytes == 0 {
            continue;
        }
        taken.push(path.clone());
        entries.push(entry(path, kind, size));
    }
    if let Some(simulators) = simulator_entry(home, settings, stop) {
        entries.push(simulators);
    }
    entries
}

/// What a Claude `projects` folder offers: the logs inside each project folder (session files
/// and folders), never its `memory` folder, which keeps the persistent agent memory. `None`
/// when `folder` is not such a folder.
pub fn session_log_items(folder: &Path) -> Option<Vec<PathBuf>> {
    let is_claude_projects = folder.file_name().is_some_and(|name| name == "projects")
        && folder
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name.to_string_lossy().starts_with(CLAUDE_PREFIX));
    if !is_claude_projects {
        return None;
    }
    let mut items = Vec::new();
    for project in fs::read_dir(folder).into_iter().flatten().flatten() {
        let path = project.path();
        let is_folder = fs::symlink_metadata(&path).is_ok_and(|found| found.file_type().is_dir());
        if !is_folder {
            items.push(path);
            continue;
        }
        for log in fs::read_dir(&path).into_iter().flatten().flatten() {
            if log.file_name() != "memory" {
                items.push(log.path());
            }
        }
    }
    Some(items)
}

/// The size of the session logs without the `memory` folders.
fn session_log_size(logs: &Path, stop: &AtomicBool) -> Option<ncrs_core::FolderSize> {
    let Some(items) = session_log_items(logs) else {
        return measure_folder(logs, stop);
    };
    let mut total = ncrs_core::FolderSize {
        bytes: 0,
        partial: false,
    };
    for path in items {
        let size = match fs::symlink_metadata(&path) {
            Ok(found) if found.file_type().is_dir() => measure_folder(&path, stop)?,
            Ok(found) => ncrs_core::FolderSize {
                bytes: found.len(),
                partial: false,
            },
            Err(_unreadable) => continue,
        };
        total.bytes = total.bytes.saturating_add(size.bytes);
        total.partial |= size.partial;
    }
    Some(total)
}

fn entry(path: PathBuf, kind: DecideKind, size: ncrs_core::FolderSize) -> Entry {
    let modified = fs::metadata(&path).and_then(|meta| meta.modified()).ok();
    Entry {
        device: device_of_path(&path),
        path,
        group: Group::Decide,
        kind: EntryKind::Decide(kind),
        size,
        modified,
        held_by: Vec::new(),
        facts: EntryFacts {
            last_used: modified,
            ..EntryFacts::default()
        },
    }
}

fn session_log_folders(home: &Path) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = fs::read_dir(home)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|child| {
            child
                .file_name()
                .to_string_lossy()
                .starts_with(CLAUDE_PREFIX)
        })
        .map(|child| child.path().join("projects"))
        .collect();
    folders.sort();
    folders.extend(
        SESSION_LOG_FOLDERS
            .iter()
            .map(|relative| home.join(relative)),
    );
    folders
}

fn model_folders(home: &Path) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = MODEL_FOLDERS
        .iter()
        .map(|relative| home.join(relative))
        .collect();
    for root in [home.join("Library/Caches"), home.join(".cache")] {
        let mut named: Vec<PathBuf> = fs::read_dir(root)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|child| {
                child
                    .file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .contains(MODEL_WORD)
            })
            .map(|child| child.path())
            .collect();
        named.sort();
        folders.extend(named);
    }
    folders
}

/// The simulators that can still be used, as one entry at the CoreSimulator folder.
/// Its size is the available devices only; the folder as a whole may not go to the trash.
fn simulator_entry(home: &Path, settings: &SpaceSettings, stop: &AtomicBool) -> Option<Entry> {
    if !cfg!(all(target_os = "macos", not(feature = "sandbox"))) {
        return None;
    }
    let folder = home.join(CORE_SIMULATOR);
    let devices = folder.join("Devices");
    if !fs::symlink_metadata(&devices).is_ok_and(|found| found.file_type().is_dir())
        || settings
            .never_touch
            .iter()
            .any(|never| folder.starts_with(never))
    {
        return None;
    }
    let bytes = device_bytes(&devices, Availability::Available, stop)?;
    (bytes > 0).then(|| {
        entry(
            folder,
            DecideKind::Simulators,
            ncrs_core::FolderSize {
                bytes,
                partial: false,
            },
        )
    })
}
