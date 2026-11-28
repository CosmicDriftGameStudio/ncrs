use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use gix::ObjectId;

use crate::cleanup::remote_tips;
use crate::repo_access::{default_branch, open, DefaultBranch};
use crate::status::{check_stop, count_commits_strict, distance_to_upstream};
use crate::{status, FileState, GitError};

/// One local branch with its distance to its upstream.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BranchInfo {
    pub name: String,
    pub is_current: bool,
    pub has_upstream: bool,
    /// Distance to the upstream (G8): commits only here, commits only there.
    pub ahead: u32,
    pub behind: u32,
    /// Everything on it is already in the default branch, local or on origin.
    /// Never set for the default branch itself, and never on a history that
    /// cannot be walked to the end.
    pub merged: bool,
    /// The short tracking name, like `origin/feature`, when one is configured.
    pub upstream: Option<String>,
    /// A tracking name is configured but its ref is gone (deleted on the remote, pruned).
    pub upstream_gone: bool,
    /// The full id of the tip.
    pub tip: String,
    /// Seconds since the Unix epoch.
    pub tip_time: i64,
    /// Commits not in the default branch; zero when `merged` or unknown.
    pub unmerged_commits: u32,
}

/// One worktree of the repository, the main one included.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
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
    /// Every commit is in main and nothing is changed or untracked; never true for the main worktree.
    pub merged: bool,
    pub is_main: bool,
    pub is_current: bool,
    /// The short tracking name of its branch, when one is configured and still exists.
    pub upstream: Option<String>,
    /// Distance of its branch to that upstream (G8); zero without one.
    pub upstream_ahead: u32,
    pub upstream_behind: u32,
    /// `git worktree lock` was used: git refuses to remove it.
    pub locked: bool,
    /// The full id of HEAD.
    pub head: Option<String>,
    /// Ignored files (not folders) by their path in the worktree: git removes a
    /// linked worktree without a word about them. Empty for the main worktree.
    pub ignored_files: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RefsOverview {
    /// The default branch: the target of `origin/HEAD`, else main, else master.
    pub main_branch: Option<String>,
    /// Its tip, the local one if there is a local branch.
    pub main_tip: Option<String>,
    pub local: Vec<BranchInfo>,
    pub remote: Vec<String>,
    /// The full ids of the tips of `remote`.
    pub remote_tips: BTreeMap<String, String>,
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
    let main = default_branch(&repository);
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
        let tracking = repository
            .branch_remote_tracking_ref_name(full_name.as_ref(), gix::remote::Direction::Fetch)
            .and_then(Result::ok);
        let upstream_gone = tracking
            .as_ref()
            .is_some_and(|tracked| repository.find_reference(tracked.as_ref()).is_err());
        let is_main = main.as_ref().is_some_and(|found| found.name == name);
        let unmerged_commits = match &main {
            Some(found) if !is_main => unmerged_against(&repository, tip, found, stop)?,
            _ => None,
        };
        local.push(BranchInfo {
            is_current: current.as_deref() == Some(name.as_str()),
            name,
            has_upstream: tracking.is_some(),
            ahead: upstream_distance.0,
            behind: upstream_distance.1,
            merged: unmerged_commits == Some(0),
            upstream: tracking.map(|tracked| tracked.shorten().to_string()),
            upstream_gone,
            tip: tip.to_string(),
            tip_time: commit_time(&repository, tip),
            unmerged_commits: unmerged_commits.unwrap_or(0),
        });
    }
    local.sort_by(|a, b| a.name.cmp(&b.name));

    let remote = names_of(platform.remote_branches().map_err(GitError::read)?)
        .into_iter()
        .filter(|name| !name.ends_with("/HEAD"))
        .collect();
    let tags = names_of(platform.tags().map_err(GitError::read)?);

    Ok(RefsOverview {
        main_branch: main.as_ref().map(|found| found.name.clone()),
        main_tip: main
            .as_ref()
            .and_then(DefaultBranch::tip)
            .map(|id| id.to_string()),
        local,
        remote,
        remote_tips: remote_tips(root, stop)?,
        tags,
        stashes: stash_count(&repository),
        worktrees: worktrees(&repository, root, main.as_ref(), stop)?,
    })
}

/// The fewest commits of `tip` that are in neither the local nor the remote
/// default branch; `None` when the history cannot be walked.
fn unmerged_against(
    repository: &gix::Repository,
    tip: ObjectId,
    default: &DefaultBranch,
    stop: &Arc<AtomicBool>,
) -> Result<Option<u32>, GitError> {
    let mut fewest: Option<u32> = None;
    for hidden in [default.local_tip, default.remote_tip]
        .into_iter()
        .flatten()
    {
        if let Some(count) = count_commits_strict(repository, tip, hidden, stop)? {
            fewest = Some(fewest.map_or(count, |known| known.min(count)));
        }
    }
    Ok(fewest)
}

fn commit_time(repository: &gix::Repository, id: ObjectId) -> i64 {
    repository
        .find_commit(id)
        .ok()
        .and_then(|commit| commit.time().ok())
        .map_or(0, |time| time.seconds)
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
    main: Option<&DefaultBranch>,
    stop: &Arc<AtomicBool>,
) -> Result<Vec<WorktreeInfo>, GitError> {
    let current = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let main_folder = crate::summary::main_worktree_of(root).unwrap_or_else(|| current.clone());
    let mut found = Vec::new();
    let all = repository
        .worktrees_including_main()
        .map_err(GitError::read)?;
    for candidate in all {
        check_stop(stop)?;
        let Ok(worktree) = candidate else { continue };
        let Some(path) = worktree.workdir().map(std::path::Path::to_path_buf) else {
            continue;
        };
        if !path.exists() {
            continue;
        }
        let canonical = dunce::canonicalize(&path).unwrap_or_else(|_| path.clone());
        let is_main = canonical == main_folder;
        let Ok(state) = status(&path, stop) else {
            continue;
        };
        let head_id = worktree
            .head()
            .ok()
            .and_then(|head| head.id().map(|id| id.detach()));
        let main_tip = main.and_then(DefaultBranch::tip);
        let (ahead, behind) = match (is_main, head_id, main_tip) {
            (false, Some(head), Some(main_id)) => (
                count_commits_strict(repository, head, main_id, stop)?.unwrap_or(0),
                count_commits_strict(repository, main_id, head, stop)?.unwrap_or(0),
            ),
            _ => (0, 0),
        };
        let changed = state
            .entries
            .values()
            .filter(|entry| **entry != FileState::Ignored)
            .count();
        let merged = match (is_main, head_id, main) {
            (false, Some(head), Some(default)) => {
                changed == 0 && unmerged_against(repository, head, default, stop)? == Some(0)
            }
            _ => false,
        };
        let (upstream, upstream_ahead, upstream_behind) =
            worktree_upstream(repository, state.branch.as_deref(), head_id, stop)?;
        let ignored_files = if is_main {
            Vec::new()
        } else {
            ignored_files(&path, &state.entries)
        };
        found.push(WorktreeInfo {
            ignored_files,
            wip: changed,
            branch: state.branch,
            head_short: state.head_short,
            ahead,
            behind,
            merged,
            is_main,
            is_current: canonical == current,
            upstream,
            upstream_ahead,
            upstream_behind,
            locked: !is_main && worktree.git_dir().join("locked").exists(),
            head: head_id.map(|id| id.to_string()),
            path,
        });
    }
    Ok(found)
}

/// The status shows an ignored folder as one entry, so folders (build output,
/// dependencies) are told apart by what is on the disk.
fn ignored_files(folder: &Path, entries: &HashMap<PathBuf, FileState>) -> Vec<String> {
    let mut files: Vec<String> = entries
        .iter()
        .filter(|(_, state)| **state == FileState::Ignored)
        .filter(|(path, _)| {
            std::fs::symlink_metadata(folder.join(path)).is_ok_and(|found| found.is_file())
        })
        .map(|(path, _)| path.to_string_lossy().into_owned())
        .collect();
    files.sort();
    files
}

fn worktree_upstream(
    repository: &gix::Repository,
    branch: Option<&str>,
    head: Option<ObjectId>,
    stop: &Arc<AtomicBool>,
) -> Result<(Option<String>, u32, u32), GitError> {
    let (Some(branch_name), Some(head_id)) = (branch, head) else {
        return Ok((None, 0, 0));
    };
    let Ok(full_name) = gix::refs::FullName::try_from(format!("refs/heads/{branch_name}")) else {
        return Ok((None, 0, 0));
    };
    let Some(Ok(tracking)) = repository
        .branch_remote_tracking_ref_name(full_name.as_ref(), gix::remote::Direction::Fetch)
    else {
        return Ok((None, 0, 0));
    };
    if repository.find_reference(tracking.as_ref()).is_err() {
        return Ok((None, 0, 0));
    }
    let (ahead, behind) = distance_to_upstream(repository, full_name.as_ref(), head_id, stop)?;
    Ok((Some(tracking.shorten().to_string()), ahead, behind))
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
