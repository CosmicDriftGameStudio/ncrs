//! Turning a path typed into the path field into a place to go.

use std::io;
use std::path::{Component, Path, PathBuf};

/// Where a typed path leads: the directory to show and the entry to put the
/// cursor on, which is set when the path names a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    pub directory: PathBuf,
    pub select: Option<String>,
}

/// Why a typed path leads nowhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathProblem {
    Missing,
    /// The path may exist, but the system would not say: no access, a stale
    /// network mount. The text is the system's own.
    Unreadable {
        reason: String,
    },
}

/// The typed text as an absolute path: `~` is `home`, a relative path starts in
/// `base`, `.` and `..` are folded away without asking the filesystem.
///
/// Lexical on purpose: `canonicalize` would follow symlinks and rewrite what the
/// user typed (`/Volumes/Data` into some other path), and fail for a path that
/// is not there, which this has to report as typed.
pub fn resolve(base: &Path, home: &Path, typed: &str) -> PathBuf {
    let expanded = if typed == "~" {
        home.to_path_buf()
    } else if let Some(rest) = typed.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(typed)
    };
    fold_dots(&base.join(expanded))
}

fn fold_dots(path: &Path) -> PathBuf {
    let mut folded: Vec<Component<'_>> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match folded.last() {
                Some(Component::Normal(_)) => {
                    folded.pop();
                }
                // `..` above the root stays at the root; on a relative path
                // with nothing to remove it is kept.
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => folded.push(component),
            },
            other => folded.push(other),
        }
    }
    folded.into_iter().collect()
}

/// Looks at `target` and says where to go. Blocks, so it runs on the blocking
/// pool through [`locate`].
fn inspect(target: &Path) -> Result<Destination, PathProblem> {
    let metadata = std::fs::metadata(target).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => PathProblem::Missing,
        _ => PathProblem::Unreadable {
            reason: error.to_string(),
        },
    })?;
    Ok(match target.parent().zip(target.file_name()) {
        Some((parent, name)) if !metadata.is_dir() => Destination {
            directory: parent.to_path_buf(),
            select: Some(name.to_string_lossy().into_owned()),
        },
        _ => Destination {
            directory: target.to_path_buf(),
            select: None,
        },
    })
}

/// [`inspect`] on tokio's blocking pool, so a slow mount cannot freeze the UI.
pub async fn locate(target: PathBuf) -> Result<Destination, PathProblem> {
    tokio::task::spawn_blocking(move || inspect(&target))
        .await
        .unwrap_or_else(|join_error| {
            Err(PathProblem::Unreadable {
                reason: join_error.to_string(),
            })
        })
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("ncrs-typed-path-")
            .tempdir()
            .expect("a scratch directory")
    }

    #[test]
    fn a_directory_is_opened_and_a_file_is_shown_in_its_folder() {
        let dir = scratch();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("file.txt"), "x").unwrap();
        assert_eq!(
            inspect(&sub),
            Ok(Destination {
                directory: sub.clone(),
                select: None
            })
        );
        assert_eq!(
            inspect(&sub.join("file.txt")),
            Ok(Destination {
                directory: sub,
                select: Some("file.txt".to_string())
            })
        );
    }

    #[test]
    fn a_missing_path_is_missing_and_not_unreadable() {
        let dir = scratch();
        assert_eq!(
            inspect(&dir.path().join("nowhere")),
            Err(PathProblem::Missing)
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_path_behind_a_closed_directory_is_unreadable_not_missing() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = scratch();
        let closed = dir.path().join("closed");
        std::fs::create_dir(&closed).unwrap();
        std::fs::write(closed.join("inside"), "x").unwrap();
        std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o000)).unwrap();
        let outcome = inspect(&closed.join("inside"));
        std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o755)).unwrap();
        // Running as root sees through the mode; there is nothing to assert then.
        if let Err(problem) = outcome {
            assert!(matches!(problem, PathProblem::Unreadable { .. }));
        }
    }

    #[tokio::test]
    async fn locate_gives_the_same_answer_off_the_calling_thread() {
        let dir = scratch();
        assert_eq!(
            locate(dir.path().to_path_buf()).await,
            Ok(Destination {
                directory: dir.path().to_path_buf(),
                select: None
            })
        );
    }

    fn resolved(typed: &str) -> PathBuf {
        resolve(Path::new("/base/dir"), Path::new("/home/me"), typed)
    }

    #[test]
    fn tilde_is_home_and_a_relative_path_starts_in_the_base() {
        assert_eq!(resolved("~"), PathBuf::from("/home/me"));
        assert_eq!(resolved("~/docs"), PathBuf::from("/home/me/docs"));
        assert_eq!(resolved("docs"), PathBuf::from("/base/dir/docs"));
        assert_eq!(resolved("/abs/path/"), PathBuf::from("/abs/path"));
    }

    #[test]
    fn dot_is_dropped_and_dot_dot_removes_the_component_before_it() {
        assert_eq!(resolved("./docs/./x"), PathBuf::from("/base/dir/docs/x"));
        assert_eq!(resolved(".."), PathBuf::from("/base"));
        assert_eq!(resolved("../other"), PathBuf::from("/base/other"));
        assert_eq!(resolved("/a/b/../c"), PathBuf::from("/a/c"));
        assert_eq!(resolved("/a/b/../../.."), PathBuf::from("/"));
        assert_eq!(resolved("~/docs/.."), PathBuf::from("/home/me"));
    }

    #[test]
    fn symlinks_are_not_resolved() {
        let dir = scratch();
        let target = dir.path().join("target");
        std::fs::create_dir(&target).unwrap();
        #[cfg(unix)]
        {
            let link = dir.path().join("link");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            let typed = format!("{}/./sub/..", link.display());
            assert_eq!(resolve(Path::new("/"), Path::new("/"), &typed), link);
        }
    }
}
