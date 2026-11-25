//! Looking up the folder of a panel on the disk, which blocks.

use std::path::{Path, PathBuf};

/// Blocking. The canonical path of the folder a panel shows: the rules
/// compare canonical paths, and the intake works on them too.
pub fn resolve_folder(folder: &Path) -> Result<PathBuf, String> {
    let canonical = ncrs_intake::canonical_path(folder).map_err(|err| err.to_string())?;
    if !canonical.is_dir() {
        return Err(std::io::Error::from(std::io::ErrorKind::NotADirectory).to_string());
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_is_resolved_to_its_canonical_path() {
        let base = tempfile::tempdir().unwrap();
        let real = base.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let file = base.path().join("file.txt");
        std::fs::write(&file, "").unwrap();

        let spelled_round_about = base.path().join("real").join(".");
        assert_eq!(
            resolve_folder(&spelled_round_about).unwrap(),
            ncrs_intake::canonical_path(&real).unwrap()
        );
        resolve_folder(&file).unwrap_err();
        resolve_folder(&base.path().join("missing")).unwrap_err();
    }
}
