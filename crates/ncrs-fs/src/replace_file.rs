use std::path::{Path, PathBuf};

/// The body of `replace_file`, shared with the session state. The path in the
/// error says which one it belongs to: the folder when the temporary file could
/// not be created, the target otherwise.
pub fn replace_file_atomically(
    target: &Path,
    temp_prefix: &str,
    content: &str,
) -> Result<(), (PathBuf, std::io::Error)> {
    use std::io::Write as _;
    let folder = target.parent().unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::Builder::new()
        .prefix(temp_prefix)
        .suffix(".tmp")
        .tempfile_in(folder)
        .map_err(|err| (folder.to_path_buf(), err))?;
    if let Ok(previous) = std::fs::metadata(target) {
        temporary
            .as_file()
            .set_permissions(previous.permissions())
            .map_err(|err| (target.to_path_buf(), err))?;
    }
    temporary
        .write_all(content.as_bytes())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|err| (target.to_path_buf(), err))?;
    temporary
        .persist(target)
        .map_err(|err| (target.to_path_buf(), err.error))?;
    Ok(())
}
