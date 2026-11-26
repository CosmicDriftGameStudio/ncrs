use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::cli::Git;
use crate::refname::plain_ref;
use crate::refs::{BranchInfo, RefsOverview, WorktreeInfo};
use crate::repo_access::{default_branch, open, parse_id};
use crate::status::count_commits_strict;
use crate::undo::{RecreatedBranch, UndoAction, UndoRecord, UndoSummary};
use crate::{ActionError, GitError};

/// Why a branch may go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanupReason {
    /// Everything on it is in the default branch.
    InDefault,
    /// Its remote branch is gone (merged there, say) and nothing was added after the last push.
    UpstreamGone,
}

/// Why something stays, even though it looks done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeepReason {
    DefaultBranch,
    CurrentBranch,
    /// Newly made and still empty (G24): nothing was done on it yet.
    Empty,
    /// Commits that are neither in the default branch nor on the remote.
    Unpushed {
        commits: u32,
    },
    /// Pushed, but not in the default branch.
    NotMerged,
    /// The remote branch is gone but the state before the fetch is not known,
    /// or the branch moved after the last push.
    AfterLastPush,
    WorktreeChanges {
        count: usize,
    },
    MainWorktree,
    CurrentWorktree,
    Detached,
    Locked,
    /// Its branch is checked out in a worktree that stays.
    InKeptWorktree,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CleanupBranch {
    pub name: String,
    /// The tip when it was scanned; the branch is only deleted while it is still there.
    pub sha: String,
    pub reason: CleanupReason,
    pub upstream: Option<String>,
    pub tip_time: i64,
    /// The tip of its remote-tracking branch when that one may be deleted on the
    /// remote (see [`RemoteState`]); the delete only goes through while the
    /// remote branch is still there.
    pub remote_tip: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CleanupWorktree {
    pub path: PathBuf,
    /// Deleted with the folder.
    pub branch: CleanupBranch,
    /// Ignored files that go with the folder, see [`WorktreeInfo::ignored_files`].
    pub ignored_files: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kept {
    /// A branch name, or the folder of a worktree.
    pub name: String,
    pub is_worktree: bool,
    pub why: KeepReason,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CleanupList {
    pub branches: Vec<CleanupBranch>,
    pub worktrees: Vec<CleanupWorktree>,
    pub kept: Vec<Kept>,
}

impl CleanupList {
    /// Branches offered, those of the offered worktrees included.
    pub fn cleanable_count(&self) -> usize {
        self.branches.len() + self.worktrees.len()
    }
}

/// What the person ticked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CleanupPlan {
    pub branches: Vec<CleanupBranch>,
    pub worktrees: Vec<CleanupWorktree>,
    /// Also delete the remote branch of the `InDefault` branches that have one.
    pub delete_remote: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CleanupTarget {
    Worktree(PathBuf),
    Branch(String),
    RemoteBranch(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CleanupRow {
    pub target: CleanupTarget,
    pub result: Result<(), ActionError>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CleanupReport {
    pub rows: Vec<CleanupRow>,
    /// Brings back the deleted branches (not worktree folders, not remote branches).
    pub undo: Option<UndoRecord>,
}

impl CleanupReport {
    pub fn all_done(&self) -> bool {
        self.rows.iter().all(|row| row.result.is_ok())
    }
}

/// The tips of the remote-tracking branches by short name (`origin/feature`).
/// Take it before a fetch: it is what tells a branch that was merged on the
/// remote from one that got new commits after its last push.
pub fn remote_tips(root: &Path, stop: &AtomicBool) -> Result<BTreeMap<String, String>, GitError> {
    let repository = open(root)?;
    let platform = repository.references().map_err(GitError::read)?;
    let mut tips = BTreeMap::new();
    for reference in platform
        .remote_branches()
        .map_err(GitError::read)?
        .flatten()
    {
        crate::status::check_stop(stop)?;
        let name = reference.name().shorten().to_string();
        if name.ends_with("/HEAD") {
            continue;
        }
        if let Ok(id) = reference.into_fully_peeled_id() {
            tips.insert(name, id.to_string());
        }
    }
    Ok(tips)
}

/// What the scan knows about the remote. Deleting a branch on the remote needs
/// all of it, so `Default` (nothing known) never deletes there.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemoteState {
    /// Fetching this repository worked in this run, so the tracking branches are current.
    pub fetched: bool,
    /// Tracking branches (`origin/feature`, with the tip) already inside the
    /// remote's default branch.
    pub in_default: BTreeMap<String, String>,
}

/// Reads the tracking branches that are fully inside the remote's default branch.
/// Call it after the fetch and pass whether the fetch worked.
pub fn remote_state(
    root: &Path,
    fetched: bool,
    stop: &AtomicBool,
) -> Result<RemoteState, GitError> {
    let mut state = RemoteState {
        fetched,
        in_default: BTreeMap::new(),
    };
    if !fetched {
        return Ok(state);
    }
    let repository = open(root)?;
    let Some(hidden) = default_branch(&repository).and_then(|found| found.remote_tip) else {
        return Ok(state);
    };
    for (name, tip) in remote_tips(root, stop)? {
        let Ok(id) = parse_id(&tip) else { continue };
        if count_commits_strict(&repository, id, hidden, stop)? == Some(0) {
            state.in_default.insert(name, tip);
        }
    }
    Ok(state)
}

/// The remote branch may go with the local one only when it is the same name,
/// is not the default branch, is no other branch's upstream, and its tip is
/// inside the default branch.
fn remote_tip_to_delete(
    branch: &BranchInfo,
    overview: &RefsOverview,
    remote: &RemoteState,
) -> Option<String> {
    let upstream = branch.upstream.as_deref()?;
    let (_, short) = upstream.split_once('/')?;
    let shared = overview
        .local
        .iter()
        .any(|other| other.name != branch.name && other.upstream.as_deref() == Some(upstream));
    if !remote.fetched
        || short != branch.name
        || overview.main_branch.as_deref() == Some(short)
        || shared
    {
        return None;
    }
    remote.in_default.get(upstream).cloned()
}

fn candidate(
    branch: &BranchInfo,
    reason: CleanupReason,
    remote_tip: Option<String>,
) -> CleanupBranch {
    CleanupBranch {
        name: branch.name.clone(),
        sha: branch.tip.clone(),
        reason,
        upstream: branch.upstream.clone(),
        tip_time: branch.tip_time,
        remote_tip,
    }
}

fn judge_branch(
    branch: &BranchInfo,
    overview: &RefsOverview,
    remote_tips_before: &BTreeMap<String, String>,
    remote: &RemoteState,
) -> Result<CleanupBranch, KeepReason> {
    if overview.main_branch.as_deref() == Some(branch.name.as_str()) {
        return Err(KeepReason::DefaultBranch);
    }
    if branch.is_current {
        return Err(KeepReason::CurrentBranch);
    }
    if branch.merged {
        let untouched =
            !branch.has_upstream && overview.main_tip.as_deref() == Some(branch.tip.as_str());
        return if untouched {
            Err(KeepReason::Empty)
        } else {
            Ok(candidate(
                branch,
                CleanupReason::InDefault,
                remote_tip_to_delete(branch, overview, remote),
            ))
        };
    }
    if branch.upstream_gone {
        let known = branch
            .upstream
            .as_ref()
            .and_then(|name| remote_tips_before.get(name));
        return match known {
            Some(tip) if *tip == branch.tip => {
                Ok(candidate(branch, CleanupReason::UpstreamGone, None))
            }
            _ => Err(KeepReason::AfterLastPush),
        };
    }
    if !branch.has_upstream {
        return Err(KeepReason::Unpushed {
            commits: branch.unmerged_commits,
        });
    }
    if branch.ahead > 0 {
        return Err(KeepReason::Unpushed {
            commits: branch.ahead,
        });
    }
    Err(KeepReason::NotMerged)
}

fn judge_worktree(
    worktree: &WorktreeInfo,
    overview: &RefsOverview,
    remote_tips_before: &BTreeMap<String, String>,
    remote: &RemoteState,
) -> Result<CleanupWorktree, KeepReason> {
    if worktree.is_main {
        return Err(KeepReason::MainWorktree);
    }
    if worktree.is_current {
        return Err(KeepReason::CurrentWorktree);
    }
    let Some(name) = worktree.branch.as_deref() else {
        return Err(KeepReason::Detached);
    };
    if worktree.locked {
        return Err(KeepReason::Locked);
    }
    if worktree.wip > 0 {
        return Err(KeepReason::WorktreeChanges {
            count: worktree.wip,
        });
    }
    let Some(branch) = overview.local.iter().find(|branch| branch.name == name) else {
        return Err(KeepReason::Detached);
    };
    // Its branch is checked out in that folder, so the branch rules apply
    // except "current", which is about the checkout of the repository itself.
    let seen_from_outside = BranchInfo {
        is_current: false,
        ..branch.clone()
    };
    judge_branch(&seen_from_outside, overview, remote_tips_before, remote).map(|judged| {
        CleanupWorktree {
            path: worktree.path.clone(),
            branch: judged,
            ignored_files: worktree.ignored_files.clone(),
        }
    })
}

/// Sorts branches and worktrees into what can go and what stays with its reason.
/// Pure: it reads only the overview and the remote tips taken before the fetch.
pub fn cleanup_list(
    overview: &RefsOverview,
    remote_tips_before: &BTreeMap<String, String>,
    remote: &RemoteState,
) -> CleanupList {
    let mut list = CleanupList::default();
    for worktree in overview.worktrees.iter().filter(|found| !found.is_main) {
        match judge_worktree(worktree, overview, remote_tips_before, remote) {
            Ok(row) => list.worktrees.push(row),
            Err(why) => list.kept.push(Kept {
                name: worktree.path.display().to_string(),
                is_worktree: true,
                why,
            }),
        }
    }
    let checked_out_elsewhere: Vec<&str> = overview
        .worktrees
        .iter()
        .filter(|found| !found.is_main)
        .filter_map(|found| found.branch.as_deref())
        .collect();
    for branch in &overview.local {
        if checked_out_elsewhere.contains(&branch.name.as_str()) {
            let offered = list
                .worktrees
                .iter()
                .any(|row| row.branch.name == branch.name);
            if !offered {
                list.kept.push(Kept {
                    name: branch.name.clone(),
                    is_worktree: false,
                    why: KeepReason::InKeptWorktree,
                });
            }
            continue;
        }
        match judge_branch(branch, overview, remote_tips_before, remote) {
            Ok(row) => list.branches.push(row),
            Err(why) => list.kept.push(Kept {
                name: branch.name.clone(),
                is_worktree: false,
                why,
            }),
        }
    }
    list
}

impl Git {
    /// Deletes what the plan names, worktrees first, then branches, then (when
    /// asked) their remote branches. A worktree git refuses to remove keeps its
    /// branch. A branch that moved since the scan stays.
    pub fn clean_up(&self, plan: &CleanupPlan) -> Result<CleanupReport, ActionError> {
        let mut rows = Vec::new();
        let mut deleted: Vec<RecreatedBranch> = Vec::new();
        let mut remote_deletes: Vec<(String, String, String)> = Vec::new();
        let mut doomed: Vec<&CleanupBranch> = Vec::new();
        for worktree in &plan.worktrees {
            let removed = self
                .ok(&["worktree", "remove", "--", &worktree.path.to_string_lossy()])
                .map(|_| ());
            let went = removed.is_ok();
            rows.push(CleanupRow {
                target: CleanupTarget::Worktree(worktree.path.clone()),
                result: removed,
            });
            if went {
                doomed.push(&worktree.branch);
            }
        }
        doomed.extend(plan.branches.iter());
        for branch in doomed {
            let result = plain_ref(&branch.name).and_then(|_| self.delete_scanned_branch(branch));
            if result.is_ok() {
                let tracking_exists = branch.upstream.as_ref().filter(|name| {
                    self.try_text(&[
                        "rev-parse",
                        "--verify",
                        "-q",
                        &format!("refs/remotes/{name}"),
                    ])
                    .is_some()
                });
                if plan.delete_remote {
                    let target = branch.remote_tip.as_ref().zip(
                        branch
                            .upstream
                            .as_deref()
                            .and_then(|name| name.split_once('/')),
                    );
                    if let Some((tip, (remote, name))) = target {
                        remote_deletes.push((remote.to_string(), name.to_string(), tip.clone()));
                    }
                }
                deleted.push(RecreatedBranch {
                    name: branch.name.clone(),
                    sha: branch.sha.clone(),
                    upstream: tracking_exists.cloned(),
                });
            }
            rows.push(CleanupRow {
                target: CleanupTarget::Branch(branch.name.clone()),
                result,
            });
        }
        for (remote, branch, tip) in remote_deletes {
            rows.push(CleanupRow {
                target: CleanupTarget::RemoteBranch(format!("{remote}/{branch}")),
                result: self.delete_remote_branch(&remote, &branch, Some(&tip)),
            });
        }
        let undo = (!deleted.is_empty()).then(|| UndoRecord {
            summary: UndoSummary::CleanUp {
                branches: deleted.len(),
            },
            description: format!(
                "Clean up {} branch(es); deleted remote branches and worktree folders stay gone",
                deleted.len()
            ),
            action: UndoAction::RecreateBranches(deleted),
        });
        Ok(CleanupReport { rows, undo })
    }

    /// `branch -D` only while the branch is still at the scanned commit:
    /// `-d` compares with the upstream or HEAD and is wrong for both a gone
    /// upstream and a branch that is not checked out here.
    fn delete_scanned_branch(&self, branch: &CleanupBranch) -> Result<(), ActionError> {
        parse_id(&branch.sha).map_err(|_unreadable| ActionError::Other {
            git_output: format!("{} has no scanned commit", branch.name),
        })?;
        let now = self.try_text(&[
            "rev-parse",
            "--verify",
            "-q",
            &format!("refs/heads/{}", branch.name),
        ]);
        if now.as_deref() != Some(branch.sha.as_str()) {
            return Err(ActionError::Other {
                git_output: format!("{} changed since the scan", branch.name),
            });
        }
        self.ok(&["branch", "-D", "--", &branch.name])?;
        Ok(())
    }
}
