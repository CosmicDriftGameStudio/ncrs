//! Total size of a folder, as the measuring walk reports it.

/// What a walk found under one folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FolderSize {
    pub bytes: u64,
    /// Something below the folder could not be read, so `bytes` is a lower bound.
    pub partial: bool,
}
