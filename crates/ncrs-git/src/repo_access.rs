use std::path::Path;

use gix::ObjectId;

use crate::GitError;

const SHORT_HASH_LENGTH: usize = 7;

/// Opens the repository without the commands its own config names: a
/// repository from an unzipped download carries its own `.git/config`, and
/// showing a folder must never execute what that file names. Drivers from the
/// user's and the system's config stay, so LFS and similar filters still apply.
pub fn open(root: &Path) -> Result<gix::Repository, GitError> {
    let mut repository = gix::open(root).map_err(GitError::open)?;
    drop_repository_command_drivers(&mut repository);
    Ok(repository)
}

fn drop_repository_command_drivers(repository: &mut gix::Repository) {
    let mut snapshot = repository.config_snapshot_mut();
    let drivers: Vec<_> = snapshot
        .sections_and_ids()
        .filter(|(section, _)| {
            let header = section.header();
            let name = header.name();
            section.meta().source.kind() == gix::config::source::Kind::Repository
                && header.subsection_name().is_some()
                && (name.eq_ignore_ascii_case(b"filter") || name.eq_ignore_ascii_case(b"diff"))
        })
        .map(|(_, id)| id)
        .collect();
    for id in drivers {
        snapshot.remove_section_by_id(id);
    }
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
