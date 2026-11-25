use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use gix::ObjectId;

use crate::refs::{RefKind, RefLabel};
use crate::repo_access::{open, parse_id};
use crate::status::check_stop;
use crate::GitError;

/// The local and remote branches that contain the commit. Loaded on its own
/// so the commit detail does not wait for it: it asks every branch.
pub fn branches_containing(
    root: &Path,
    sha: &str,
    stop: &Arc<AtomicBool>,
) -> Result<Vec<RefLabel>, GitError> {
    check_stop(stop)?;
    let repository = open(root)?;
    let wanted = parse_id(sha)?;
    let platform = repository.references().map_err(GitError::read)?;
    let mut found = Vec::new();
    let sources = [
        (platform.local_branches().map_err(GitError::read)?, RefKind::Branch),
        (platform.remote_branches().map_err(GitError::read)?, RefKind::Remote),
    ];
    for (references, kind) in sources {
        for reference in references.flatten() {
            check_stop(stop)?;
            let name = reference.name().shorten().to_string();
            if name.ends_with("/HEAD") {
                continue;
            }
            let Ok(tip) = reference.into_fully_peeled_id().map(|id| id.detach()) else {
                continue;
            };
            if contains(&repository, tip, wanted) {
                found.push(RefLabel { name, kind });
            }
        }
    }
    found.sort_by(|a, b| (a.kind as u8, &a.name).cmp(&(b.kind as u8, &b.name)));
    Ok(found)
}

fn contains(repository: &gix::Repository, tip: ObjectId, wanted: ObjectId) -> bool {
    tip == wanted
        || repository
            .merge_base(tip, wanted)
            .is_ok_and(|base| base.is_some_and(|found| found.detach() == wanted))
}
