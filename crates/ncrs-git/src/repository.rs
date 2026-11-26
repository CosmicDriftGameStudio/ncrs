use std::path::{Path, PathBuf};

/// A folder is a repository root when it has a `.git` folder, or a `.git` file as
/// linked worktrees and submodules do. Only one metadata call, so it is cheap
/// enough for every folder row.
pub fn is_repository_root(dir: &Path) -> bool {
    dir.join(".git").exists()
}

/// The closest folder at or above `path` that is a repository root.
pub fn repository_root_of(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|candidate| is_repository_root(candidate))
        .map(Path::to_path_buf)
}

/// Where `file` is below the repository root `root`, with `/` between the
/// parts as git has it; `None` for a file that is not below it.
pub fn path_in_repository(root: &Path, file: &Path) -> Option<String> {
    let inner = match file.strip_prefix(root) {
        Ok(inner) => inner.to_path_buf(),
        // `/tmp` and `/private/tmp`, a short name or another case: the folders
        // are compared as the disk has them, the file itself may be a link.
        Err(_) => std::fs::canonicalize(file.parent()?)
            .ok()?
            .join(file.file_name()?)
            .strip_prefix(std::fs::canonicalize(root).ok()?)
            .ok()?
            .to_path_buf(),
    };
    let parts: Vec<String> = inner
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "test code: a failing setup is the signal"
)]
mod tests {
    use super::path_in_repository;

    #[test]
    fn a_file_is_named_below_its_repository_with_slashes() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("src").join("main.rs");
        assert_eq!(
            path_in_repository(dir.path(), &file).as_deref(),
            Some("src/main.rs")
        );
        assert_eq!(path_in_repository(dir.path(), dir.path()), None);
        let elsewhere = tempfile::tempdir().unwrap();
        assert_eq!(path_in_repository(elsewhere.path(), &file), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_file_reached_through_a_linked_folder_is_still_found() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir_all(real.join("src")).unwrap();
        std::fs::write(real.join("src/main.rs"), "x").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(
            path_in_repository(&real, &link.join("src/main.rs")).as_deref(),
            Some("src/main.rs")
        );
    }
}
