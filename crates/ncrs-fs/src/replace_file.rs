use std::path::{Path, PathBuf};

/// The body of `replace_file`, shared with the session state. The path in the
/// error says which one it belongs to: the folder when the temporary file could
/// not be created, the target otherwise.
///
/// A target that is a symlink is followed: the new content goes next to the
/// real file and replaces that, so the link stays a link.
pub fn replace_file_atomically(
    target: &Path,
    temp_prefix: &str,
    content: &str,
) -> Result<(), (PathBuf, std::io::Error)> {
    use std::io::Write as _;
    let real = std::fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());
    let folder = real.parent().unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::Builder::new()
        .prefix(temp_prefix)
        .suffix(".tmp")
        .tempfile_in(folder)
        .map_err(|err| (folder.to_path_buf(), err))?;
    if let Ok(previous) = std::fs::metadata(&real) {
        temporary
            .as_file()
            .set_permissions(previous.permissions())
            .map_err(|err| (target.to_path_buf(), err))?;
        let _unreported = crate::place_file::copy_xattrs(&real, temporary.path());
    }
    temporary
        .write_all(content.as_bytes())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|err| (target.to_path_buf(), err))?;
    temporary
        .persist(&real)
        .map_err(|err| (target.to_path_buf(), err.error))?;
    // The rename is only durable once the folder entry is. Not every platform
    // can open a folder to sync it, and the file is complete either way.
    #[cfg(unix)]
    if let Ok(handle) = std::fs::File::open(folder) {
        let _best_effort = handle.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_symlinked_file_stays_a_link_and_its_target_gets_the_content() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.toml");
        let link = dir.path().join("link.toml");
        std::fs::write(&real, "old").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        replace_file_atomically(&link, ".cfg-", "new").unwrap();

        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "new");
    }

    #[test]
    fn a_new_file_is_created() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("fresh.toml");

        replace_file_atomically(&target, ".cfg-", "x").unwrap();

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "x");
    }
}
