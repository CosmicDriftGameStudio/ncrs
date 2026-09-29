use std::io;
use std::path::PathBuf;

/// Creates one directory.
///
/// Fails if anything already exists at `path`: silently succeeding would hide
/// a mistyped name, and silently reusing an existing directory would put files
/// where the user did not ask for them.
pub async fn create_dir(path: PathBuf) -> Result<(), CreateDirError> {
    let for_error = path.clone();
    tokio::task::spawn_blocking(move || std::fs::create_dir(&path))
        .await
        .map_err(|join| CreateDirError::TaskFailed {
            reason: join.to_string(),
        })?
        .map_err(|source| CreateDirError::AlreadyExists {
            path: for_error,
            source,
        })
}

/// Why a directory could not be created. Carries no user-facing text; the UI
/// layer formats it in the active language.
#[derive(Debug)]
pub enum CreateDirError {
    /// The filesystem refused. `source` distinguishes the cases a user needs
    /// told apart — the name is taken, or the parent does not exist — and both
    /// arrive here from `create_dir`.
    AlreadyExists { path: PathBuf, source: io::Error },
    /// The background task failed, not the filesystem operation.
    TaskFailed { reason: String },
}

/// `io::Error` is not `Clone` or `PartialEq`, and `Message` needs both, so the
/// comparison is on the kind of failure rather than the whole error. That is
/// also the comparison the UI makes: "name taken" and "parent gone" are
/// different messages, every `io::ErrorKind` within one is the same message.
impl Clone for CreateDirError {
    fn clone(&self) -> Self {
        match self {
            CreateDirError::AlreadyExists { path, source } => CreateDirError::AlreadyExists {
                path: path.clone(),
                source: io::Error::new(source.kind(), source.to_string()),
            },
            CreateDirError::TaskFailed { reason } => CreateDirError::TaskFailed {
                reason: reason.clone(),
            },
        }
    }
}

impl PartialEq for CreateDirError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                CreateDirError::AlreadyExists {
                    path: a,
                    source: sa,
                },
                CreateDirError::AlreadyExists {
                    path: b,
                    source: sb,
                },
            ) => a == b && sa.kind() == sb.kind(),
            (
                CreateDirError::TaskFailed { reason: a },
                CreateDirError::TaskFailed { reason: b },
            ) => a == b,
            _ => false,
        }
    }
}

impl Eq for CreateDirError {}

impl CreateDirError {
    /// The OS error, when there was one. The UI needs it to tell "that name is
    /// taken" apart from "the parent directory is gone" — two different fixes
    /// for the user.
    pub fn io_error(&self) -> Option<&io::Error> {
        match self {
            CreateDirError::AlreadyExists { source, .. } => Some(source),
            CreateDirError::TaskFailed { .. } => None,
        }
    }
}

impl std::fmt::Display for CreateDirError {
    /// Names the failure, not the fix. The user-facing wording is built by the
    /// UI layer in the active language; this exists for logs.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CreateDirError::AlreadyExists { path, .. } => {
                write!(f, "{} already exists", path.display())
            }
            CreateDirError::TaskFailed { reason } => write!(f, "task failed: {reason}"),
        }
    }
}

impl std::error::Error for CreateDirError {}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
// reason: a test module belongs next to what it tests
#[allow(clippy::inline_modules)]
#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory that removes itself, including when the test fails
    /// halfway. A hand-made path in `temp_dir()` survives a panic and makes the
    /// next run fail with `AlreadyExists` for no reason.
    fn scratch_dir(label: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("ncrs-{label}-"))
            .tempdir()
            .expect("a scratch directory")
    }

    #[tokio::test]
    async fn creates_the_directory() {
        let scratch = scratch_dir("ops-create");
        let base = scratch.path().to_path_buf();
        let target = base.join("newdir");
        std::fs::create_dir_all(&base).unwrap();

        create_dir(target.clone()).await.unwrap();

        assert!(target.is_dir());
    }

    /// The failure that matters: a name that is already taken must not be
    /// reported as success, or the file operations built on top of this would
    /// quietly write into the wrong place.
    #[tokio::test]
    async fn refuses_an_existing_directory() {
        let scratch = scratch_dir("ops-exists");
        let base = scratch.path().to_path_buf();
        std::fs::create_dir_all(&base).unwrap();

        let err = create_dir(base.clone()).await.unwrap_err();
        assert!(
            matches!(err, CreateDirError::AlreadyExists { .. }),
            "expected AlreadyExists, got {err:?}"
        );
        assert!(base.is_dir(), "the existing directory should be untouched");
    }

    /// A file in the way counts as "already exists" too.
    #[tokio::test]
    async fn refuses_when_a_file_is_in_the_way() {
        let scratch = scratch_dir("ops-file-in-the-way");
        let base = scratch.path().to_path_buf();
        std::fs::create_dir_all(&base).unwrap();
        let target = base.join("afile");
        std::fs::write(&target, b"content").unwrap();

        let err = create_dir(target.clone()).await.unwrap_err();
        assert!(matches!(err, CreateDirError::AlreadyExists { .. }));
        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"content",
            "the file must not be replaced by a directory"
        );
    }

    /// A parent that does not exist is a different failure from "name taken",
    /// and the user needs to be told which one happened: the fix is different.
    #[tokio::test]
    async fn a_missing_parent_is_not_reported_as_a_taken_name() {
        let scratch = scratch_dir("ops-missing-parent");
        let base = scratch.path().to_path_buf();
        let target = base.join("nope").join("deeper");

        let err = create_dir(target).await.unwrap_err();
        let io_error = err.io_error().expect("a filesystem failure");
        assert_eq!(
            io_error.kind(),
            std::io::ErrorKind::NotFound,
            "expected NotFound, got {io_error:?} ({err})"
        );
    }

    /// A name that is taken reports the other kind, so the UI can say so.
    #[tokio::test]
    async fn a_taken_name_reports_already_exists() {
        let scratch = scratch_dir("ops-taken");
        let base = scratch.path().to_path_buf();
        std::fs::create_dir_all(&base).unwrap();

        let err = create_dir(base.clone()).await.unwrap_err();
        assert_eq!(
            err.io_error().unwrap().kind(),
            std::io::ErrorKind::AlreadyExists
        );
    }

    /// Two calls with the same name: the second must fail. This is the case a
    /// retry or a double keypress produces.
    #[tokio::test]
    async fn a_second_call_with_the_same_name_fails() {
        let scratch = scratch_dir("ops-twice");
        let base = scratch.path().to_path_buf();
        let target = base.join("once");
        std::fs::create_dir_all(&base).unwrap();

        create_dir(target.clone()).await.unwrap();
        assert!(create_dir(target.clone()).await.is_err());
    }
}
