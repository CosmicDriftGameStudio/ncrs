use std::path::Path;

use gix::ObjectId;

use crate::GitError;

const SHORT_HASH_LENGTH: usize = 7;

pub fn open(root: &Path) -> Result<gix::Repository, GitError> {
    gix::open(root).map_err(GitError::open)
}

pub fn short_hash(id: &ObjectId) -> String {
    id.to_string().chars().take(SHORT_HASH_LENGTH).collect()
}

pub fn parse_id(sha: &str) -> Result<ObjectId, GitError> {
    ObjectId::from_hex(sha.as_bytes()).map_err(GitError::read)
}

/// The branch the others are measured against: `main`, else `master`.
pub fn main_branch(repository: &gix::Repository) -> Option<(String, ObjectId)> {
    ["main", "master"].into_iter().find_map(|name| {
        let reference = repository
            .find_reference(format!("refs/heads/{name}").as_str())
            .ok()?;
        let id = reference.into_fully_peeled_id().ok()?.detach();
        Some((name.to_string(), id))
    })
}
