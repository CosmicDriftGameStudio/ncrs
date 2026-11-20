#![allow(dead_code, reason = "each test file uses a part of the shared helpers")]

use std::path::{Path, PathBuf};

/// The filing setup's check, which the chat crate cannot depend on.
pub fn resolve_folder(folder: &Path) -> Result<PathBuf, String> {
    let canonical = ncrs_intake::canonical_path(folder).map_err(|err| err.to_string())?;
    if canonical.is_dir() {
        Ok(canonical)
    } else {
        Err("not a directory".to_owned())
    }
}
