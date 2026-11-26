//! The push of a bulk run. It is the push of the Git tab: the repository's
//! hooks run, git may ask for a login through `job`, and nothing kills it, so
//! a pre-push hook can take its time. `Git::open_unattended` would switch the
//! hooks off, which is a push without its check.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use ncrs_git::{ActionError, Git, RepoSummary};

use crate::bulk::{
    error_line, plan_for, BulkAction, Executed, Facts, Outcome, Plan, Ran, SkipReason,
};
use crate::summary;

/// What git's answer to a push makes of the row: no login is its own result,
/// everything else is a failure with the last line git said.
fn outcome_of_push_error(error: &ActionError) -> Outcome {
    match error {
        ActionError::AuthFailed { remote, .. } => Outcome::NoAccess {
            remote: remote.clone(),
        },
        other => Outcome::Failed {
            last_line: error_line(other),
        },
    }
}

/// Pushes the checked-out branch of the repository at `path`; blocks.
pub fn execute(
    path: &Path,
    before: &RepoSummary,
    is_worktree: bool,
    job: u64,
    stop: &Arc<AtomicBool>,
) -> Executed {
    let command = BulkAction::Push.command().to_owned();
    let ended = |outcome: Outcome, ran: Vec<Ran>, after| Executed {
        outcome,
        ran,
        after,
    };
    match plan_for(BulkAction::Push, &Facts::of(before, is_worktree)) {
        Plan::Skip(reason) => return ended(Outcome::Skipped(reason), Vec::new(), None),
        Plan::Nothing => return ended(Outcome::AlreadyCurrent, Vec::new(), None),
        Plan::Run => {}
    }
    let git = match Git::open_interactive(path, job) {
        Ok(git) => git,
        Err(error) => {
            let ran = vec![Ran {
                command,
                result: error_line(&error),
            }];
            return ended(outcome_of_push_error(&error), ran, None);
        }
    };
    // The summary can be older than the last command in a terminal.
    if git.current_branch().is_none() {
        return ended(Outcome::Skipped(SkipReason::DetachedHead), Vec::new(), None);
    }
    let (outcome, result) = match git.push() {
        Ok(()) => (
            Outcome::Pushed {
                ahead: before.ahead,
            },
            "ok".to_owned(),
        ),
        Err(error) => (outcome_of_push_error(&error), error_line(&error)),
    };
    let ran = vec![Ran { command, result }];
    ended(outcome, ran, Some(summary::read(path, stop)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rejected_push_fails_with_the_line_git_said() {
        assert_eq!(
            outcome_of_push_error(&ActionError::NotFastForward {
                git_output: "! [rejected] main -> main (fetch first)\n".to_owned()
            }),
            Outcome::Failed {
                last_line: "! [rejected] main -> main (fetch first)".to_owned()
            }
        );
        assert!(matches!(
            outcome_of_push_error(&ActionError::Rejected { behind: 2 }),
            Outcome::Failed { .. }
        ));
    }

    #[test]
    fn a_missing_login_is_no_access() {
        assert_eq!(
            outcome_of_push_error(&ActionError::AuthFailed {
                remote: "origin".to_owned(),
                git_output: "denied".to_owned()
            }),
            Outcome::NoAccess {
                remote: "origin".to_owned()
            }
        );
    }

    fn git(dir: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// A clone one commit ahead of its bare remote, with a pre-push hook that
    /// leaves a mark and ends with `exit`.
    #[cfg(unix)]
    fn clone_ahead_with_hook(root: &Path, exit: u8) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt as _;
        let remote = root.join("remote.git");
        let work = root.join("work");
        let (remote_arg, work_arg) = (remote.to_str().unwrap(), work.to_str().unwrap());
        git(root, &["init", "--bare", "-q", remote_arg]);
        git(root, &["clone", "-q", remote_arg, work_arg]);
        git(&work, &["commit", "-q", "--allow-empty", "-m", "first"]);
        git(&work, &["push", "-q", "-u", "origin", "HEAD"]);
        git(&work, &["commit", "-q", "--allow-empty", "-m", "second"]);
        let hook = work.join(".git/hooks/pre-push");
        let script = format!("#!/bin/sh\ntouch hook-ran\nexit {exit}\n");
        std::fs::write(&hook, script).unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        work
    }

    fn push_of(work: &Path) -> Executed {
        let stop = Arc::new(AtomicBool::new(false));
        let summary::Reading::Read(before) = summary::read(work, &stop) else {
            panic!("the clone reads");
        };
        assert_eq!(before.ahead, 1);
        execute(work, &before, false, 0, &stop)
    }

    #[cfg(unix)]
    #[test]
    fn a_push_runs_the_pre_push_hook_and_reaches_the_remote() {
        let root = tempfile::tempdir().unwrap();
        let work = clone_ahead_with_hook(root.path(), 0);
        let executed = push_of(&work);
        assert_eq!(executed.outcome, Outcome::Pushed { ahead: 1 });
        assert!(work.join("hook-ran").exists());
        let Some(summary::Reading::Read(after)) = executed.after else {
            panic!("the repo is read again");
        };
        assert_eq!(after.ahead, 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_pre_push_hook_that_fails_stops_the_push() {
        let root = tempfile::tempdir().unwrap();
        let work = clone_ahead_with_hook(root.path(), 1);
        let executed = push_of(&work);
        assert!(
            matches!(executed.outcome, Outcome::Failed { .. }),
            "{:?}",
            executed.outcome
        );
        assert!(work.join("hook-ran").exists());
    }
}
