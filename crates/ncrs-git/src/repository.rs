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
