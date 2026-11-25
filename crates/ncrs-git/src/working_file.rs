use std::fs;
use std::io::{BufRead as _, BufReader, Read as _};
use std::path::{Component, Path};

use crate::diff::MAX_DIFFED_BYTES;
use crate::undo::refuse_symlinks;
use crate::ActionError;

/// Whether the read below stopped before the end of the file.
fn is_cut(bytes: &[u8]) -> bool {
    bytes.len() > MAX_DIFFED_BYTES
}

/// The whole text of a file of the working copy; `None` where it is no text or
/// too large to be read whole.
pub fn read_working_text(root: &Path, relative: &str) -> Option<String> {
    let bytes = read_working_file(root, relative).filter(|read| !is_cut(read))?;
    String::from_utf8(bytes).ok()
}

/// The whole file, for an action that writes back what it read: a file that
/// would be cut is refused.
pub(crate) fn read_whole_working_file(root: &Path, relative: &str) -> Result<Vec<u8>, ActionError> {
    let bytes = read_working_file(root, relative).ok_or_else(|| ActionError::Other {
        git_output: format!("{relative} cannot be read"),
    })?;
    refuse_cut(relative, &bytes)?;
    Ok(bytes)
}

pub(crate) fn refuse_cut(relative: &str, bytes: &[u8]) -> Result<(), ActionError> {
    if is_cut(bytes) {
        return Err(ActionError::Other {
            git_output: format!("{relative} is too large for this: use the editor"),
        });
    }
    Ok(())
}

/// Whether a text file that is too large to be read whole still has an opening
/// conflict marker.
pub(crate) fn large_text_has_conflict_marker(root: &Path, relative: &str) -> bool {
    let is_large_text = read_working_file(root, relative)
        .is_some_and(|bytes| is_cut(&bytes) && !bytes.contains(&0));
    if !is_large_text {
        return false;
    }
    let Ok(file) = fs::File::open(root.join(relative)) else {
        return false;
    };
    BufReader::new(file)
        .split(b'\n')
        .map_while(Result::ok)
        .any(|line| line.starts_with(b"<<<<<<< "))
}

/// Whether the file is text whose conflict markers are all gone, also where it
/// is too large to be read whole.
pub fn is_text_without_markers(root: &Path, relative: &str) -> bool {
    match read_working_file(root, relative) {
        Some(bytes) if is_cut(&bytes) => {
            !bytes.contains(&0) && !large_text_has_conflict_marker(root, relative)
        }
        Some(bytes) => String::from_utf8(bytes)
            .is_ok_and(|text| !text.lines().any(|line| line.starts_with("<<<<<<< "))),
        None => false,
    }
}

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
    if let Some(parent) = path
        .parent()
        .and_then(Path::to_str)
        .filter(|p| !p.is_empty())
    {
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
