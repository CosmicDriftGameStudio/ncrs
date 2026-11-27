use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

use ncrs_core::FolderSize;
use ncrs_git::{
    cleanup_list, main_worktree_of, refs, CleanupPlan, CleanupTarget, Git, RemoteState,
};

use crate::checks::{check_subject, ProcessSource, Skip, Subject, UnknownUse};
use crate::log::{LogRow, RunLog};

/// A linked worktree whose branch is merged, without local changes or unpushed commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeSpace {
    /// The main folder of the repository the worktree belongs to.
    pub repo: PathBuf,
    pub path: PathBuf,
    pub branch: String,
    pub size: FolderSize,
    pub modified: Option<SystemTime>,
}

/// Blocking. Reads only what is known locally, no fetch. A repository that cannot be read is left out.
pub fn merged_worktrees(repos: &[PathBuf], stop: &AtomicBool) -> Vec<WorktreeSpace> {
    // ponytail: git reads one repository without a stop; `stop` is checked between repositories and worktrees.
    let unstoppable = Arc::new(AtomicBool::new(false));
    let mut found = Vec::new();
    for repo in main_folders(repos) {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        for worktree in merged_in(&repo, &unstoppable) {
            let Some(size) = ncrs_fs::measure_folder(&worktree.path, stop) else {
                return found;
            };
            let modified = std::fs::symlink_metadata(&worktree.path)
                .and_then(|metadata| metadata.modified())
                .ok();
            found.push(WorktreeSpace {
                repo: repo.clone(),
                branch: worktree.branch.name,
                path: worktree.path,
                size,
                modified,
            });
        }
    }
    found
}

/// Blocking. Checks each worktree again, then removes it through git. Git refuses a
/// worktree that has changes, and the branch is judged merged again right before.
pub fn remove_worktrees(
    worktrees: &[WorktreeSpace],
    probes: &dyn ProcessSource,
    unknown: UnknownUse,
) -> RunLog {
    let mut log = RunLog::new(SystemTime::now());
    let stop = Arc::new(AtomicBool::new(false));
    for worktree in worktrees {
        let outcome = remove_one(worktree, probes, unknown, &stop);
        log.rows.push(LogRow {
            path: worktree.path.clone(),
            size: worktree.size.bytes,
            outcome,
        });
    }
    log.finished = SystemTime::now();
    log
}

fn remove_one(
    worktree: &WorktreeSpace,
    probes: &dyn ProcessSource,
    unknown: UnknownUse,
    stop: &Arc<AtomicBool>,
) -> Result<u64, Skip> {
    check_subject(
        &Subject {
            path: &worktree.path,
            tools: &[],
            bytes: Some(worktree.size.bytes),
            modified: worktree.modified,
            device: None,
        },
        probes,
        unknown,
    )?;
    let still_merged = merged_in(&worktree.repo, stop)
        .into_iter()
        .find(|candidate| candidate.path == worktree.path)
        .ok_or(Skip::Changed)?;
    let git = Git::open(&worktree.repo).map_err(|error| Skip::Failed(error.to_string()))?;
    let report = git
        .clean_up(&CleanupPlan { worktrees: vec![still_merged], ..CleanupPlan::default() })
        .map_err(|error| Skip::Failed(error.to_string()))?;
    let row = report
        .rows
        .into_iter()
        .find(|row| matches!(&row.target, CleanupTarget::Worktree(path) if *path == worktree.path))
        .ok_or_else(|| Skip::Failed("git reported nothing".to_owned()))?;
    row.result.map_err(|error| Skip::Failed(error.to_string()))?;
    Ok(worktree.size.bytes)
}

fn merged_in(repo: &Path, stop: &Arc<AtomicBool>) -> Vec<ncrs_git::CleanupWorktree> {
    let Ok(overview) = refs(repo, stop) else {
        return Vec::new();
    };
    cleanup_list(&overview, &BTreeMap::new(), &RemoteState::default()).worktrees
}

/// The main folder of each repository, once, however it was listed.
fn main_folders(repos: &[PathBuf]) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = Vec::new();
    for repo in repos {
        let main = main_worktree_of(repo)
            .or_else(|| std::fs::canonicalize(repo).ok())
            .unwrap_or_else(|| repo.clone());
        if !folders.contains(&main) {
            folders.push(main);
        }
    }
    folders
}
