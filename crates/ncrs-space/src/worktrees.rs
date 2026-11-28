use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

use ncrs_core::FolderSize;
use ncrs_git::{
    cleanup_list, main_worktree_of, refs, CleanupList, CleanupPlan, CleanupTarget, Git, KeepReason,
    RefsOverview, RemoteState,
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
    /// Git-ignored files (`.env`, local notes) that `git worktree remove` would delete for good.
    pub ignored_files: Vec<String>,
}

/// Why a linked worktree stays although it exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeptWhy {
    /// Its branch is not merged yet, or has commits nobody else has.
    Open,
    /// It has changes that are not committed.
    Changes,
    /// It is merged and clean, but a process runs in it or has a file open.
    InUse,
    /// It is merged and clean, but holds git-ignored files that git would delete for good.
    IgnoredFiles,
    /// Locked, detached or the folder ncrs itself works in.
    Other,
}

impl KeptWhy {
    fn of(reason: KeepReason) -> Self {
        match reason {
            KeepReason::NotMerged
            | KeepReason::Unpushed { .. }
            | KeepReason::AfterLastPush
            | KeepReason::Empty => Self::Open,
            KeepReason::WorktreeChanges { .. } => Self::Changes,
            KeepReason::DefaultBranch
            | KeepReason::CurrentBranch
            | KeepReason::MainWorktree
            | KeepReason::CurrentWorktree
            | KeepReason::Detached
            | KeepReason::Locked
            | KeepReason::InKeptWorktree => Self::Other,
        }
    }
}

/// A linked worktree that is not offered for removal, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeptWorktree {
    pub repo: PathBuf,
    pub path: PathBuf,
    pub branch: Option<String>,
    pub size: FolderSize,
    pub modified: Option<SystemTime>,
    pub why: KeptWhy,
}

/// Every linked worktree of the repositories: the merged ones and the ones that stay.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorktreeScan {
    pub merged: Vec<WorktreeSpace>,
    pub kept: Vec<KeptWorktree>,
}

/// How many worktrees, and how much space they take.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorktreeCount {
    pub count: usize,
    pub bytes: u64,
}

impl WorktreeCount {
    fn add(&mut self, bytes: u64) {
        self.count += 1;
        self.bytes = self.bytes.saturating_add(bytes);
    }
}

/// The worktrees below one folder (such as `~/code/.wt`): all of them, those that can go,
/// and those that stay, each with its reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeRoot {
    pub folder: PathBuf,
    pub total: WorktreeCount,
    /// Merged, clean and free: what the worktree button removes.
    pub merged: WorktreeCount,
    pub kept: WorktreeCount,
    pub kept_list: Vec<KeptWorktree>,
}

impl WorktreeRoot {
    pub fn kept_because(&self, why: KeptWhy) -> WorktreeCount {
        let mut count = WorktreeCount::default();
        for kept in self.kept_list.iter().filter(|kept| kept.why == why) {
            count.add(kept.size.bytes);
        }
        count
    }
}

/// Groups worktrees by the folder they lie in. `in_use` holds the paths of merged ones that a
/// process holds; those count as staying, with [`KeptWhy::InUse`]. Biggest folder first.
pub fn worktree_roots(
    merged: &[WorktreeSpace],
    in_use: &HashSet<PathBuf>,
    kept: &[KeptWorktree],
) -> Vec<WorktreeRoot> {
    let mut roots: BTreeMap<PathBuf, WorktreeRoot> = BTreeMap::new();
    for worktree in merged {
        let root = roots
            .entry(parent_folder(&worktree.path))
            .or_insert_with(|| new_root(&worktree.path));
        root.total.add(worktree.size.bytes);
        if in_use.contains(&worktree.path) {
            root.kept.add(worktree.size.bytes);
            root.kept_list.push(KeptWorktree {
                repo: worktree.repo.clone(),
                path: worktree.path.clone(),
                branch: Some(worktree.branch.clone()),
                size: worktree.size,
                modified: worktree.modified,
                why: KeptWhy::InUse,
            });
        } else {
            root.merged.add(worktree.size.bytes);
        }
    }
    for worktree in kept {
        let root = roots
            .entry(parent_folder(&worktree.path))
            .or_insert_with(|| new_root(&worktree.path));
        root.total.add(worktree.size.bytes);
        root.kept.add(worktree.size.bytes);
        root.kept_list.push(worktree.clone());
    }
    let mut found: Vec<WorktreeRoot> = roots.into_values().collect();
    found.sort_by_key(|left| std::cmp::Reverse(left.total.bytes));
    found
}

fn parent_folder(path: &Path) -> PathBuf {
    path.parent().map(Path::to_path_buf).unwrap_or_default()
}

fn new_root(path: &Path) -> WorktreeRoot {
    WorktreeRoot {
        folder: parent_folder(path),
        total: WorktreeCount::default(),
        merged: WorktreeCount::default(),
        kept: WorktreeCount::default(),
        kept_list: Vec::new(),
    }
}

/// Blocking. Reads only what is known locally, no fetch. A repository that cannot be read is left out.
pub fn merged_worktrees(repos: &[PathBuf], stop: &AtomicBool) -> Vec<WorktreeSpace> {
    collect(repos, stop, false).merged
}

/// [`merged_worktrees`] and the worktrees that stay, measured, with their reasons.
pub fn scan_worktrees(repos: &[PathBuf], stop: &AtomicBool) -> WorktreeScan {
    collect(repos, stop, true)
}

fn collect(repos: &[PathBuf], stop: &AtomicBool, with_kept: bool) -> WorktreeScan {
    // ponytail: git reads one repository without a stop; `stop` is checked between repositories and worktrees.
    let unstoppable = Arc::new(AtomicBool::new(false));
    let mut found = WorktreeScan::default();
    for repo in main_folders(repos) {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let Some((list, overview)) = judged_in(&repo, &unstoppable) else {
            continue;
        };
        for worktree in list.worktrees {
            let Some(size) = ncrs_fs::measure_folder(&worktree.path, stop) else {
                return found;
            };
            let modified = modified_of(&worktree.path);
            if !worktree.ignored_files.is_empty() {
                if with_kept {
                    found.kept.push(KeptWorktree {
                        repo: repo.clone(),
                        branch: Some(worktree.branch.name),
                        path: worktree.path,
                        size,
                        modified,
                        why: KeptWhy::IgnoredFiles,
                    });
                }
                continue;
            }
            found.merged.push(WorktreeSpace {
                repo: repo.clone(),
                branch: worktree.branch.name,
                path: worktree.path,
                size,
                modified,
                ignored_files: worktree.ignored_files,
            });
        }
        if !with_kept {
            continue;
        }
        for kept in list.kept.into_iter().filter(|kept| kept.is_worktree) {
            let path = PathBuf::from(&kept.name);
            let Some(size) = ncrs_fs::measure_folder(&path, stop) else {
                return found;
            };
            found.kept.push(KeptWorktree {
                repo: repo.clone(),
                branch: overview
                    .worktrees
                    .iter()
                    .find(|worktree| worktree.path == path)
                    .and_then(|worktree| worktree.branch.clone()),
                modified: modified_of(&path),
                why: KeptWhy::of(kept.why),
                path,
                size,
            });
        }
    }
    found
}

fn modified_of(path: &Path) -> Option<SystemTime> {
    std::fs::symlink_metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

/// Blocking. Checks each worktree again, then removes it through git. Git refuses a
/// worktree that has changes, and the branch is judged merged again right before.
pub fn remove_worktrees(
    worktrees: &[WorktreeSpace],
    probes: &dyn ProcessSource,
    unknown: UnknownUse,
    stop: &Arc<AtomicBool>,
) -> RunLog {
    let mut log = RunLog::new(SystemTime::now());
    for worktree in worktrees {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let (outcome, use_checked) = remove_one(worktree, probes, unknown, stop);
        log.rows.push(LogRow {
            path: worktree.path.clone(),
            size: worktree.size.bytes,
            use_checked,
            last_used: worktree.modified,
            outcome,
        });
    }
    log.finished = SystemTime::now();
    log
}

/// The outcome, and whether the in-use check got a real answer.
fn remove_one(
    worktree: &WorktreeSpace,
    probes: &dyn ProcessSource,
    unknown: UnknownUse,
    stop: &Arc<AtomicBool>,
) -> (Result<u64, Skip>, bool) {
    let checked = check_subject(
        &Subject {
            path: &worktree.path,
            tools: &[],
            bytes: Some(worktree.size.bytes),
            modified: worktree.modified,
            device: None,
        },
        probes,
        unknown,
    );
    match checked {
        Ok(answered) => (remove_merged(worktree, stop), answered),
        Err(skip) => (Err(skip), false),
    }
}

fn remove_merged(worktree: &WorktreeSpace, stop: &Arc<AtomicBool>) -> Result<u64, Skip> {
    let still_merged = merged_in(&worktree.repo, stop)
        .into_iter()
        .find(|candidate| candidate.path == worktree.path)
        .ok_or(Skip::Changed)?;
    if !still_merged.ignored_files.is_empty() {
        return Err(Skip::Changed);
    }
    let git = Git::open(&worktree.repo).map_err(|error| Skip::Failed(error.to_string()))?;
    let report = git
        .clean_up(&CleanupPlan {
            worktrees: vec![still_merged],
            ..CleanupPlan::default()
        })
        .map_err(|error| Skip::Failed(error.to_string()))?;
    let row = report
        .rows
        .into_iter()
        .find(|row| matches!(&row.target, CleanupTarget::Worktree(path) if *path == worktree.path))
        .ok_or_else(|| Skip::Failed("git reported nothing".to_owned()))?;
    row.result
        .map_err(|error| Skip::Failed(error.to_string()))?;
    Ok(worktree.size.bytes)
}

fn merged_in(repo: &Path, stop: &Arc<AtomicBool>) -> Vec<ncrs_git::CleanupWorktree> {
    judged_in(repo, stop).map_or_else(Vec::new, |(list, _)| list.worktrees)
}

fn judged_in(repo: &Path, stop: &Arc<AtomicBool>) -> Option<(CleanupList, RefsOverview)> {
    let overview = refs(repo, stop).ok()?;
    let list = cleanup_list(&overview, &BTreeMap::new(), &RemoteState::default());
    Some((list, overview))
}

/// The main folder of each repository, once, however it was listed.
fn main_folders(repos: &[PathBuf]) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = Vec::new();
    for repo in repos {
        let main = main_worktree_of(repo)
            .or_else(|| dunce::canonicalize(repo).ok())
            .unwrap_or_else(|| repo.clone());
        if !folders.contains(&main) {
            folders.push(main);
        }
    }
    folders
}
