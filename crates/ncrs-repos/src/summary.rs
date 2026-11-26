//! One repository read for the list: its branch, distance to the remote,
//! local changes and worktrees.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use ncrs_git::RepoSummary;

/// What reading a listed folder gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reading {
    /// The folder is not there any more.
    Gone,
    /// It is there but git cannot read it; why.
    Unreadable(String),
    Read(Box<RepoSummary>),
}

impl Reading {
    pub fn summary(&self) -> Option<&RepoSummary> {
        match self {
            Self::Read(summary) => Some(summary),
            Self::Gone | Self::Unreadable(_) => None,
        }
    }

    /// The row is faint: the folder is gone or cannot be read.
    pub const fn is_unreachable(&self) -> bool {
        !matches!(self, Self::Read(_))
    }

    /// Something is waiting: commits to fetch or push, or local changes.
    pub fn needs_action(&self) -> bool {
        self.summary()
            .is_some_and(|summary| summary.behind > 0 || summary.ahead > 0 || summary.changed > 0)
    }
}

/// Reads `path`; blocks.
pub fn read(path: &Path, stop: &Arc<AtomicBool>) -> Reading {
    if !path.is_dir() {
        return Reading::Gone;
    }
    match ncrs_git::repo_summary(path, stop) {
        Ok(summary) => Reading::Read(Box::new(summary)),
        Err(error) => Reading::Unreadable(error.to_string()),
    }
}
