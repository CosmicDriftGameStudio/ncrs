use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitError {
    /// The folder could not be opened as a repository.
    Open(String),
    /// Reading the repository failed after it was opened.
    Read(String),
    /// The caller raised the stop flag.
    Interrupted,
}

impl GitError {
    pub(crate) fn open(cause: impl fmt::Display) -> Self {
        Self::Open(cause.to_string())
    }

    pub(crate) fn read(cause: impl fmt::Display) -> Self {
        Self::Read(cause.to_string())
    }
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(cause) => write!(f, "cannot open the repository: {cause}"),
            Self::Read(cause) => write!(f, "cannot read the repository: {cause}"),
            Self::Interrupted => f.write_str("the git status was stopped"),
        }
    }
}

impl std::error::Error for GitError {}
