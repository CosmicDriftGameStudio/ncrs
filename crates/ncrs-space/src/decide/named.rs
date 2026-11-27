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

pub(super) fn named_entries(
    home: &Path,
    settings: &SpaceSettings,
    stop: &AtomicBool,
) -> Vec<Entry> {
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
        let never = settings
            .never_touch
            .iter()
            .any(|never| path.starts_with(never));
        if !is_real_folder || overlaps || never {
            continue;
        }
        let Some(size) = measure_folder(&path, stop) else {
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
