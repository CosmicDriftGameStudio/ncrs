use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::repo_access::{default_branch, open};
use crate::status::{check_stop, distance_to_upstream};
use crate::GitError;

/// A linked worktree of the repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreeSummary {
    pub path: PathBuf,
    pub branch: Option<String>,
}

/// The state of one repository, cheap enough for a list of dozens.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepoSummary {
    /// `None` when HEAD is detached or there is no commit yet.
    pub branch: Option<String>,
    pub default_branch: Option<String>,
    /// Distance of the checked-out branch to its upstream (G8).
    pub ahead: u32,
    pub behind: u32,
    /// The checked-out branch has an upstream whose remote branch is known here.
    pub has_upstream: bool,
    /// Changed and untracked paths, ignored ones left out.
    pub changed: usize,
    /// The last fetch, from the time of `FETCH_HEAD`.
    pub fetched_at: Option<SystemTime>,
    /// The linked worktrees; the main one is the repository itself.
    pub worktrees: Vec<WorktreeSummary>,
    /// When `root` is itself a linked worktree: the folder of the repository it belongs to.
    pub main_worktree: Option<PathBuf>,
}

/// When `root` is a linked worktree: the folder of the repository it belongs to.
pub fn main_worktree_of(root: &Path) -> Option<PathBuf> {
    main_worktree(&open(root).ok()?)
}

fn main_worktree(repository: &gix::Repository) -> Option<PathBuf> {
    let common = dunce::canonicalize(repository.common_dir()).ok()?;
    let own = dunce::canonicalize(repository.git_dir()).ok()?;
    (common != own)
        .then(|| common.parent().map(Path::to_path_buf))
        .flatten()
}

/// When the repository last fetched, or `None` when it never did.
pub fn last_fetch(root: &Path) -> Option<SystemTime> {
    let repository = open(root).ok()?;
    std::fs::metadata(repository.common_dir().join("FETCH_HEAD"))
        .and_then(|found| found.modified())
        .ok()
}

/// Reads the summary of the repository at `root`. Blocks; raising `stop` ends it
/// with `GitError::Interrupted`.
pub fn repo_summary(
    root: &Path,
    stop: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<RepoSummary, GitError> {
    check_stop(stop)?;
    let repository = open(root)?;
    let head = repository.head().map_err(GitError::read)?;
    let branch = head.referent_name().map(|name| name.shorten().to_string());
    let head_id = head.id().map(|id| id.detach());
    let (ahead, behind) = match (head.referent_name(), head_id) {
        (Some(name), Some(id)) => distance_to_upstream(&repository, name, id, stop)?,
        _ => (0, 0),
    };
    let has_upstream = head.referent_name().is_some_and(|name| {
        repository
            .branch_remote_tracking_ref_name(name, gix::remote::Direction::Fetch)
            .and_then(Result::ok)
            .is_some_and(|tracking| repository.find_reference(&tracking).is_ok())
    });
    let changed = crate::status(root, stop)?
        .entries
        .values()
        .filter(|state| **state != crate::FileState::Ignored)
        .count();
    let mut worktrees = Vec::new();
    for proxy in repository.worktrees().map_err(GitError::read)? {
        check_stop(stop)?;
        let Ok(linked) = proxy.into_repo_with_possibly_inaccessible_worktree() else {
            continue;
        };
        // Git keeps a worktree whose folder was deleted until it is pruned;
        // there is nothing to open.
        let Some(path) = linked
            .workdir()
            .filter(|folder| folder.is_dir())
            .map(Path::to_path_buf)
        else {
            continue;
        };
        let linked_branch = linked
            .head_name()
            .ok()
            .flatten()
            .map(|name| name.shorten().to_string());
        worktrees.push(WorktreeSummary {
            path,
            branch: linked_branch,
        });
    }
    let main_worktree = main_worktree(&repository);
    Ok(RepoSummary {
        branch,
        default_branch: default_branch(&repository).map(|found| found.name),
        ahead,
        behind,
        has_upstream,
        changed,
        fetched_at: last_fetch(root),
        worktrees,
        main_worktree,
    })
}
