//! `/proc/<pid>/cwd` and `/proc/<pid>/fd/*`, read as links. A process of another
//! user refuses the read, a process that ended meanwhile is gone: both are skipped.

use std::fs;
use std::path::Path;

use crate::{failed, FolderUse, Holder};

pub fn folder_use(folder: &Path) -> FolderUse {
    let canonical = fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    let processes = match fs::read_dir("/proc") {
        Ok(processes) => processes,
        Err(err) => return failed(&err),
    };
    let mut held = Vec::new();
    for process in processes.flatten() {
        let Some(pid) = process
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        if holds(&process.path(), &canonical) {
            let name = fs::read_to_string(process.path().join("comm"))
                .map(|text| text.trim().to_owned())
                .unwrap_or_default();
            held.push(Holder { pid, name });
        }
    }
    held.sort_by_key(|holder| holder.pid);
    if held.is_empty() {
        FolderUse::Free
    } else {
        FolderUse::Held(held)
    }
}

fn holds(process: &Path, folder: &Path) -> bool {
    if fs::read_link(process.join("cwd")).is_ok_and(|cwd| cwd.starts_with(folder)) {
        return true;
    }
    let Ok(descriptors) = fs::read_dir(process.join("fd")) else {
        return false;
    };
    descriptors
        .flatten()
        .any(|fd| fs::read_link(fd.path()).is_ok_and(|target| target.starts_with(folder)))
}
