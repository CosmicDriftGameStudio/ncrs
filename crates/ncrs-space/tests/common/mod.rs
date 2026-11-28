#![allow(dead_code, reason = "each test file uses part of these helpers")]

use std::path::Path;
use std::time::Duration;

use ncrs_core::FolderSize;
use ncrs_space::{
    CatalogId, Entry, EntryFacts, EntryKind, FolderUse, Group, Holder, ProcessSource,
};

/// Answers about running processes from a fixed script.
pub struct Fake {
    pub processes: Option<Vec<Holder>>,
    pub folder: FolderUse,
}

impl Fake {
    pub fn quiet() -> Self {
        Self {
            processes: Some(Vec::new()),
            folder: FolderUse::Free,
        }
    }

    pub fn running(name: &str) -> Self {
        Self {
            processes: Some(vec![Holder {
                pid: 42,
                name: name.to_owned(),
            }]),
            folder: FolderUse::Free,
        }
    }
}

impl ProcessSource for Fake {
    fn processes(&self) -> Option<Vec<Holder>> {
        self.processes.clone()
    }

    fn folder_use(&self, _folder: &Path, _limit: Duration) -> FolderUse {
        self.folder.clone()
    }
}

pub fn write(path: &Path, len: usize) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, vec![b'x'; len]).unwrap();
}

/// An entry as a scan would have recorded the folder right now.
pub fn entry_for(path: &Path, id: CatalogId) -> Entry {
    let metadata = std::fs::symlink_metadata(path).unwrap();
    let bytes = ncrs_fs::measure_folder(path, &std::sync::atomic::AtomicBool::new(false))
        .unwrap()
        .bytes;
    Entry {
        path: path.to_path_buf(),
        group: Group::Safe,
        kind: EntryKind::Catalog(id),
        size: FolderSize {
            bytes,
            partial: false,
        },
        modified: metadata.modified().ok(),
        device: device(&metadata),
        held_by: Vec::new(),
        facts: EntryFacts::default(),
    }
}

#[cfg(unix)]
pub fn device(metadata: &std::fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt as _;
    Some(metadata.dev())
}

#[cfg(not(unix))]
pub fn device(_metadata: &std::fs::Metadata) -> Option<u64> {
    None
}
