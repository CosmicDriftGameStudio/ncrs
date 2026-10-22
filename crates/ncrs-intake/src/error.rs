use std::fmt;
use std::io;
use std::path::PathBuf;

use ncrs_index::IndexError;

#[derive(Debug)]
#[non_exhaustive]
pub enum IntakeError {
    Index(IndexError),
    Io { path: PathBuf, source: io::Error },
    Watch(notify::Error),
}

impl IntakeError {
    pub fn io(failing_path: impl Into<PathBuf>) -> impl FnOnce(io::Error) -> Self {
        let path = failing_path.into();
        move |source| Self::Io { path, source }
    }
}

impl fmt::Display for IntakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Index(err) => err.fmt(f),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Watch(err) => write!(f, "cannot watch for changes: {err}"),
        }
    }
}

impl std::error::Error for IntakeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Index(err) => Some(err),
            Self::Io {
                path: _path,
                source,
            } => Some(source),
            Self::Watch(err) => Some(err),
        }
    }
}

impl From<IndexError> for IntakeError {
    fn from(err: IndexError) -> Self {
        Self::Index(err)
    }
}
