use std::fs;
use std::io::Read as _;
use std::path::{Component, Path};

use crate::diff::MAX_DIFFED_BYTES;
use crate::undo::refuse_symlinks;

/// What the working copy has at `relative`, read the way git would stage it:
/// a symlink is its link text and is not followed, only regular files are
/// read, and a path that leaves the working copy has no content. At most one
/// byte more than the diff accepts is read, so a larger file shows as too large
/// without being loaded, and a pipe cannot hang the read.
pub(crate) fn read_working_file(root: &Path, relative: &str) -> Option<Vec<u8>> {
    let path = Path::new(relative);
    if !path
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
    {
        return None;
    }
    if let Some(parent) = path.parent().and_then(Path::to_str).filter(|p| !p.is_empty()) {
        refuse_symlinks(root, parent).ok()?;
    }
    let full = root.join(path);
    let found = fs::symlink_metadata(&full).ok()?;
    if found.file_type().is_symlink() {
        let target = fs::read_link(&full).ok()?;
        return Some(target.to_string_lossy().into_owned().into_bytes());
    }
    if !found.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(&full)
        .ok()?
        .take(MAX_DIFFED_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(bytes)
}
