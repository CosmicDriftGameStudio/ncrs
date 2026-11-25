//! The blocking work of the clean-up, started on the blocking pool.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use iced::Task;
use ncrs_git::{
    cleanup_list, refs, remote_state, remote_tips, CleanupPlan, CleanupReport, CleanupRow,
    CleanupTarget, Git, RemoteState, UndoRecord,
};

use crate::bulk::error_line;
use crate::cleanup::{CleanupMessage, Scan};
use crate::message::Message;

/// What a listed repository has that can be cleaned up, from the refs as they
/// are (no fetch). Remote branches that were pruned count only once the pane
/// has proven them merged.
pub fn cleanable(path: PathBuf, stop: Arc<AtomicBool>) -> Task<Message> {
    Task::perform(
        async move {
            let to_read = path.clone();
            let count = tokio::task::spawn_blocking(move || {
                let overview = refs(&to_read, &stop).ok()?;
                Some(
                    cleanup_list(&overview, &BTreeMap::new(), &RemoteState::default())
                        .cleanable_count(),
                )
            })
            .await
            .ok()
            .flatten();
            (path, count)
        },
        |(root, count)| Message::CleanableRead(root, count),
    )
}

fn scan_blocking(path: &PathBuf, stop: &Arc<AtomicBool>) -> Result<Scan, String> {
    let git = Git::open_unattended(path).map_err(|error| error_line(&error))?;
    let tips_before = remote_tips(path, stop).unwrap_or_default();
    let fetch_error = git.fetch(None).err().map(|error| error_line(&error));
    let overview = refs(path, stop).map_err(|error| error.to_string())?;
    let remote =
        remote_state(path, fetch_error.is_none(), stop).map_err(|error| error.to_string())?;
    Ok(Scan {
        list: cleanup_list(&overview, &tips_before, &remote),
        default_branch: overview.main_branch,
        fetch_error,
    })
}

pub fn scan(generation: u64, path: PathBuf, stop: Arc<AtomicBool>) -> Task<Message> {
    Task::perform(
        async move {
            let to_scan = path.clone();
            let result = tokio::task::spawn_blocking(move || scan_blocking(&to_scan, &stop))
                .await
                .unwrap_or_else(|error| Err(error.to_string()));
            (path, result)
        },
        move |(root, result)| {
            Message::Cleanup(CleanupMessage::Scanned(generation, root, Box::new(result)))
        },
    )
}

#[derive(Debug, PartialEq, Eq)]
struct RemoteDelete {
    remote: String,
    branch: String,
    tip: String,
}

/// The remote branches of the local ones that went; a branch that stayed keeps its own.
fn remote_deletes(plan: &CleanupPlan, report: &CleanupReport) -> Vec<RemoteDelete> {
    let went = |name: &str| {
        report.rows.iter().any(|row| {
            row.result.is_ok()
                && matches!(&row.target, CleanupTarget::Branch(branch) if branch == name)
        })
    };
    plan.worktrees
        .iter()
        .map(|worktree| &worktree.branch)
        .chain(&plan.branches)
        .filter(|branch| went(&branch.name))
        .filter_map(|branch| {
            let tip = branch.remote_tip.clone()?;
            let (remote, name) = branch.upstream.as_deref()?.split_once('/')?;
            Some(RemoteDelete {
                remote: remote.to_owned(),
                branch: name.to_owned(),
                tip,
            })
        })
        .collect()
}

/// Local deletes run with the repository's filters, which `worktree remove`
/// needs to see a clean folder; only the calls to a remote are unattended, so
/// one that hangs or asks for a login ends.
fn delete_blocking(path: &Path, plan: &CleanupPlan) -> Result<CleanupReport, String> {
    let local = CleanupPlan {
        delete_remote: false,
        ..plan.clone()
    };
    let mut report = Git::open(path)
        .and_then(|git| git.clean_up(&local))
        .map_err(|error| error_line(&error))?;
    if plan.delete_remote {
        let unattended = Git::open_unattended(path);
        for delete in remote_deletes(plan, &report) {
            let result = unattended.clone().and_then(|git| {
                git.delete_remote_branch(&delete.remote, &delete.branch, Some(&delete.tip))
            });
            report.rows.push(CleanupRow {
                target: CleanupTarget::RemoteBranch(format!("{}/{}", delete.remote, delete.branch)),
                result,
            });
        }
    }
    Ok(report)
}

pub fn delete(generation: u64, path: PathBuf, plan: CleanupPlan) -> Task<Message> {
    Task::perform(
        async move {
            let to_clean = path.clone();
            let result = tokio::task::spawn_blocking(move || delete_blocking(&to_clean, &plan))
                .await
                .unwrap_or_else(|error| Err(error.to_string()));
            (path, result)
        },
        move |(root, result)| {
            Message::Cleanup(CleanupMessage::Deleted(generation, root, Box::new(result)))
        },
    )
}

pub fn undo(generation: u64, records: Vec<(PathBuf, UndoRecord)>) -> Task<Message> {
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                records
                    .into_iter()
                    .map(|(path, record)| {
                        let result = Git::open(&path)
                            .and_then(|git| git.undo(&record))
                            .map_err(|error| error_line(&error));
                        (path, result)
                    })
                    .collect::<Vec<_>>()
            })
            .await
            .unwrap_or_default()
        },
        move |results| Message::Cleanup(CleanupMessage::Undone(generation, results)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ncrs_git::{ActionError, CleanupBranch, CleanupReason, CleanupWorktree};

    fn branch(name: &str) -> CleanupBranch {
        CleanupBranch {
            name: name.to_owned(),
            sha: "abc".to_owned(),
            reason: CleanupReason::InDefault,
            upstream: Some(format!("origin/{name}")),
            tip_time: 0,
            remote_tip: Some("def".to_owned()),
        }
    }

    fn row(name: &str, result: Result<(), ActionError>) -> CleanupRow {
        CleanupRow {
            target: CleanupTarget::Branch(name.to_owned()),
            result,
        }
    }

    #[test]
    fn a_branch_that_stayed_keeps_its_remote_branch() {
        let plan = CleanupPlan {
            branches: vec![
                branch("gone"),
                branch("moved"),
                CleanupBranch {
                    remote_tip: None,
                    ..branch("not-proven")
                },
            ],
            worktrees: vec![CleanupWorktree {
                path: PathBuf::from("/code/kept"),
                branch: branch("in-kept-worktree"),
                ignored_files: Vec::new(),
            }],
            delete_remote: true,
        };
        let report = CleanupReport {
            rows: vec![
                row("gone", Ok(())),
                row(
                    "moved",
                    Err(ActionError::Other {
                        git_output: "moved changed since the scan".to_owned(),
                    }),
                ),
                row("not-proven", Ok(())),
            ],
            undo: None,
        };
        assert_eq!(
            remote_deletes(&plan, &report),
            [RemoteDelete {
                remote: "origin".to_owned(),
                branch: "gone".to_owned(),
                tip: "def".to_owned(),
            }]
        );
    }
}
