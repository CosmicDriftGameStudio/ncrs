use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use gix::ObjectId;

use crate::repo_access::{main_branch, open};
use crate::status::{check_stop, count_commits, distance_to_upstream};
use crate::{status, FileState, GitError};

/// One local branch with its distance to its upstream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchInfo {
    pub name: String,
    pub is_current: bool,
    pub has_upstream: bool,
    pub ahead: u32,
    pub behind: u32,
    /// Everything on it is already in main (or master). Never set for main itself.
    pub merged: bool,
}

/// One worktree of the repository, the main one included.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    /// `None` when HEAD is detached.
    pub branch: Option<String>,
    pub head_short: Option<String>,
    /// Changed and untracked entries, ignored ones left out.
    pub wip: usize,
    /// Distance to main; zero for the main worktree itself.
    pub ahead: u32,
    pub behind: u32,
    pub merged: bool,
    pub is_main: bool,
    pub is_current: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RefsOverview {
    pub main_branch: Option<String>,
    pub local: Vec<BranchInfo>,
    pub remote: Vec<String>,
    pub tags: Vec<String>,
    pub stashes: usize,
    pub worktrees: Vec<WorktreeInfo>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefKind {
    Branch,
    Remote,
    Tag,
}

/// A name pointing at a commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefLabel {
    pub name: String,
    pub kind: RefKind,
}

/// Branches, tags, stashes and worktrees of the repository at `root`. Read only.
pub fn refs(root: &std::path::Path, stop: &Arc<AtomicBool>) -> Result<RefsOverview, GitError> {
    check_stop(stop)?;
    let repository = open(root)?;
    let main = main_branch(&repository);
    let current = repository
        .head()
        .map_err(GitError::read)?
        .referent_name()
        .map(|name| name.shorten().to_string());

    let mut local = Vec::new();
    let platform = repository.references().map_err(GitError::read)?;
    for reference in platform.local_branches().map_err(GitError::read)?.flatten() {
        check_stop(stop)?;
        let full_name = reference.name().to_owned();
        let name = full_name.shorten().to_string();
        let Ok(tip) = reference.into_fully_peeled_id().map(|id| id.detach()) else {
            continue;
        };
        let upstream_distance = distance_to_upstream(&repository, full_name.as_ref(), tip, stop)?;
        let has_upstream = repository
            .branch_remote_tracking_ref_name(full_name.as_ref(), gix::remote::Direction::Fetch)
            .is_some_and(|tracking| tracking.is_ok());
        let is_main = main
            .as_ref()
            .is_some_and(|(main_name, _)| *main_name == name);
        let merged = match &main {
            Some((_, main_tip)) if !is_main => {
                count_commits(&repository, tip, *main_tip, stop)? == 0
            }
            _ => false,
        };
        local.push(BranchInfo {
            is_current: current.as_deref() == Some(name.as_str()),
            name,
            has_upstream,
            ahead: upstream_distance.0,
            behind: upstream_distance.1,
            merged,
        });
    }
    local.sort_by(|a, b| a.name.cmp(&b.name));

    let remote = names_of(platform.remote_branches().map_err(GitError::read)?)
        .into_iter()
        .filter(|name| !name.ends_with("/HEAD"))
        .collect();
    let tags = names_of(platform.tags().map_err(GitError::read)?);

    Ok(RefsOverview {
        main_branch: main.as_ref().map(|(name, _)| name.clone()),
        local,
        remote,
        tags,
        stashes: stash_count(&repository),
        worktrees: worktrees(&repository, root, main.as_ref().map(|(_, id)| *id), stop)?,
    })
}

fn names_of(references: gix::reference::iter::Iter<'_, '_>) -> Vec<String> {
    let mut names: Vec<String> = references
        .flatten()
        .map(|reference| reference.name().shorten().to_string())
        .collect();
    names.sort();
    names
}

fn stash_count(repository: &gix::Repository) -> usize {
    std::fs::read_to_string(repository.common_dir().join("logs/refs/stash"))
        .map(|log| log.lines().filter(|line| !line.trim().is_empty()).count())
        .unwrap_or(0)
}

fn worktrees(
    repository: &gix::Repository,
    root: &std::path::Path,
    main_tip: Option<ObjectId>,
    stop: &Arc<AtomicBool>,
) -> Result<Vec<WorktreeInfo>, GitError> {
    let current = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut found = Vec::new();
    let all = repository
        .worktrees_including_main()
        .map_err(GitError::read)?;
    for (index, candidate) in all.enumerate() {
        check_stop(stop)?;
        let Ok(worktree) = candidate else { continue };
        let Some(path) = worktree.workdir().map(std::path::Path::to_path_buf) else {
            continue;
        };
        if !path.exists() {
            continue;
        }
        let Ok(state) = status(&path, stop) else {
            continue;
        };
        let head_id = worktree
            .head()
            .ok()
            .and_then(|head| head.id().map(|id| id.detach()));
        let (ahead, behind) = match (index, head_id, main_tip) {
            (1.., Some(head), Some(main)) => (
                count_commits(repository, head, main, stop)?,
                count_commits(repository, main, head, stop)?,
            ),
            _ => (0, 0),
        };
        let is_main = index == 0;
        let canonical = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        found.push(WorktreeInfo {
            wip: state
                .entries
                .values()
                .filter(|entry| **entry != FileState::Ignored)
                .count(),
            branch: state.branch,
            head_short: state.head_short,
            ahead,
            behind,
            merged: !is_main && ahead == 0 && head_id.is_some() && main_tip.is_some(),
            is_main,
            is_current: canonical == current,
            path,
        });
    }
    Ok(found)
}

/// Which names point at which commit, for the chips of the commit list.
pub fn labels_by_commit(repository: &gix::Repository) -> HashMap<ObjectId, Vec<RefLabel>> {
    let mut labels: HashMap<ObjectId, Vec<RefLabel>> = HashMap::new();
    let Ok(platform) = repository.references() else {
        return labels;
    };
    add_labels(&mut labels, platform.local_branches(), RefKind::Branch);
    add_labels(&mut labels, platform.remote_branches(), RefKind::Remote);
    add_labels(&mut labels, platform.tags(), RefKind::Tag);
    labels
}

fn add_labels<E>(
    labels: &mut HashMap<ObjectId, Vec<RefLabel>>,
    listed: Result<gix::reference::iter::Iter<'_, '_>, E>,
    kind: RefKind,
) {
    let Ok(references) = listed else { return };
    for reference in references.flatten() {
        let name = reference.name().shorten().to_string();
        if name.ends_with("/HEAD") {
            continue;
        }
        if let Ok(id) = reference.into_fully_peeled_id() {
            labels
                .entry(id.detach())
                .or_default()
                .push(RefLabel { name, kind });
        }
    }
}
