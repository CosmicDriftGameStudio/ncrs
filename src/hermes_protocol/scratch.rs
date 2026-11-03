//! The files the run works on.

use std::path::{Path, PathBuf};

/// Copies files and folders; dot entries and symlinks stay behind.
pub(super) fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().filter_map(Result::ok) {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            copy_tree(&entry.path(), &to.join(&name));
        } else if kind.is_file() {
            std::fs::copy(entry.path(), to.join(&name)).ok();
        }
    }
}

/// The same tree as hard links, so a language's run can move files freely.
pub(super) fn link_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().filter_map(Result::ok) {
        let target = to.join(entry.file_name());
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            link_tree(&entry.path(), &target);
        } else if std::fs::hard_link(entry.path(), &target).is_err() {
            std::fs::copy(entry.path(), &target).ok();
        }
    }
}

pub(super) fn count_files(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| {
                    if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                        count_files(&entry.path())
                    } else {
                        usize::from(!entry.file_name().to_string_lossy().starts_with('.'))
                    }
                })
                .sum()
        })
        .unwrap_or(0)
}

pub(super) fn pdfs_below(dir: &Path, found: &mut Vec<PathBuf>, limit: usize) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        if found.len() >= limit {
            return;
        }
        let path = entry.path();
        if path.is_dir() {
            pdfs_below(&path, found, limit);
        } else if path.extension().is_some_and(|extension| extension == "pdf") {
            found.push(path);
        }
    }
}

/// A short stable name for a path, for the folder that keeps what a run reuses.
pub(super) fn path_hash(path: &Path) -> String {
    let hash = path
        .to_string_lossy()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
    format!("{hash:016x}")
}

/// The extensions the pipeline takes into the index; the other files of the
/// copy are never indexed, so the run must not wait for them.
const INDEXED_EXTENSIONS: [&str; 5] = ["pdf", "docx", "odt", "txt", "md"];

pub(super) fn count_indexable(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| {
                    let path = entry.path();
                    if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                        count_indexable(&path)
                    } else {
                        let hidden = entry.file_name().to_string_lossy().starts_with('.');
                        let indexed = path.extension().is_some_and(|extension| {
                            INDEXED_EXTENSIONS
                                .iter()
                                .any(|wanted| extension.eq_ignore_ascii_case(wanted))
                        });
                        usize::from(!hidden && indexed)
                    }
                })
                .sum()
        })
        .unwrap_or(0)
}
