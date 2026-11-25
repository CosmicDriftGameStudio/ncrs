//! What a bulk action does to one repository, and why it sometimes does
//! nothing. `plan_for` and `after_fetch` are pure, so every branch is tested
//! without git; `execute` runs the plan with the git layer.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use ncrs_git::{ActionError, Git, IntegrateOutcome, PullStrategy, RepoSummary};
use ncrs_ui_kit::git_state::RunOutcome;

use crate::summary::{self, Reading};

/// An action over many repositories. Another action is another variant and
/// an arm in `plan_for` and `execute`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulkAction {
    Fetch,
    SwitchToDefault,
    PullFastForward,
    Both,
}

impl BulkAction {
    pub const ALL: [Self; 4] = [
        Self::Fetch,
        Self::SwitchToDefault,
        Self::PullFastForward,
        Self::Both,
    ];

    /// The command as the log shows it.
    pub const fn command(self) -> &'static str {
        match self {
            Self::Fetch => "git fetch --prune --all",
            Self::SwitchToDefault => "git checkout <default>",
            Self::PullFastForward => "git pull --ff-only",
            Self::Both => "git checkout <default> && git pull --ff-only",
        }
    }
}

/// What is known about a repository before anything runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// A linked worktree of another repository in the list: never touched.
    pub is_worktree: bool,
    pub changed: usize,
    pub branch: Option<String>,
    pub default_branch: Option<String>,
    pub ahead: u32,
    pub behind: u32,
}

impl Facts {
    pub fn of(summary: &RepoSummary, is_worktree: bool) -> Self {
        Self {
            is_worktree,
            changed: summary.changed,
            branch: summary.branch.clone(),
            default_branch: summary.default_branch.clone(),
            ahead: summary.ahead,
            behind: summary.behind,
        }
    }

    fn off_default(&self) -> bool {
        matches!((&self.branch, &self.default_branch), (Some(branch), Some(default)) if branch != default)
            || (self.branch.is_none() && self.default_branch.is_some())
    }
}

/// Why a repository is left alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    Worktree,
    LocalChanges(usize),
    /// Local commits and remote commits: not a fast-forward.
    Diverged {
        ahead: u32,
    },
}

/// What happens to a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    Skip(SkipReason),
    /// There is nothing to do: already on the default branch, say.
    Nothing,
    Run,
}

/// The decision before the first command. A fetch changes no file of the
/// working copy, so changes do not stop it; switching and pulling do.
pub fn plan_for(action: BulkAction, facts: &Facts) -> Plan {
    if facts.is_worktree {
        return Plan::Skip(SkipReason::Worktree);
    }
    if action != BulkAction::Fetch && facts.changed > 0 {
        return Plan::Skip(SkipReason::LocalChanges(facts.changed));
    }
    match action {
        BulkAction::SwitchToDefault if !facts.off_default() => Plan::Nothing,
        _ => Plan::Run,
    }
}

/// The decision about the pull, once a fetch has brought the distance up to date.
pub fn after_fetch(facts: &Facts) -> Plan {
    match (facts.ahead, facts.behind) {
        (ahead, behind) if ahead > 0 && behind > 0 => Plan::Skip(SkipReason::Diverged { ahead }),
        (_, 0) => Plan::Nothing,
        _ => Plan::Run,
    }
}

/// How a repository ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Running,
    Updated {
        behind: u32,
    },
    AlreadyCurrent,
    Switched {
        from: String,
        to: String,
        behind: u32,
    },
    Skipped(SkipReason),
    /// No login for this remote: nothing was asked.
    NoAccess {
        remote: String,
    },
    Failed {
        last_line: String,
    },
}

impl Outcome {
    pub fn kind(&self) -> RunOutcome {
        match self {
            Self::Running => RunOutcome::Running,
            Self::Updated { .. } | Self::Switched { .. } => RunOutcome::Updated,
            Self::AlreadyCurrent => RunOutcome::AlreadyCurrent,
            Self::Skipped(_) => RunOutcome::Skipped,
            Self::NoAccess { .. } | Self::Failed { .. } => RunOutcome::Failed,
        }
    }

    /// The repo has a problem the user should look at.
    pub fn is_problem(&self) -> bool {
        matches!(self.kind(), RunOutcome::Failed | RunOutcome::Skipped)
            && !matches!(self, Self::Skipped(SkipReason::Worktree))
    }

    pub const fn is_failure(&self) -> bool {
        matches!(self, Self::NoAccess { .. } | Self::Failed { .. })
    }
}

/// One command that ran, for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    pub command: String,
    /// The last line git said, or what it came to.
    pub result: String,
}

/// What `execute` brings back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Executed {
    pub outcome: Outcome,
    pub ran: Vec<Ran>,
    /// The repository read again after the commands, for the list.
    pub after: Option<Reading>,
}

/// The last line of git's output, trimmed; what the log shows.
pub fn last_line(output: &str) -> String {
    output
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_owned()
}

/// Sorts what git said into the result a row shows.
pub fn outcome_of_error(error: &ActionError) -> Outcome {
    match error {
        ActionError::AuthFailed { remote, .. } => Outcome::NoAccess {
            remote: remote.clone(),
        },
        ActionError::NotFastForward { .. } | ActionError::Rejected { .. } => {
            Outcome::Skipped(SkipReason::Diverged { ahead: 0 })
        }
        ActionError::NetworkFailed { git_output } | ActionError::Other { git_output } => {
            Outcome::Failed {
                last_line: last_line(git_output),
            }
        }
        other => Outcome::Failed {
            last_line: last_line(&other.to_string()),
        },
    }
}

pub fn error_line(error: &ActionError) -> String {
    match error {
        ActionError::AuthFailed { git_output, .. }
        | ActionError::NetworkFailed { git_output }
        | ActionError::NotFastForward { git_output }
        | ActionError::Other { git_output } => last_line(git_output),
        other => last_line(&other.to_string()),
    }
}

/// Runs `action` on the repository at `path`; blocks. Never asks for a login.
pub fn execute(
    action: BulkAction,
    path: &Path,
    before: &RepoSummary,
    is_worktree: bool,
    stop: &Arc<AtomicBool>,
) -> Executed {
    let mut facts = Facts::of(before, is_worktree);
    let mut ran: Vec<Ran> = Vec::new();
    let finish = |outcome: Outcome, steps: Vec<Ran>| Executed {
        after: None,
        outcome,
        ran: steps,
    };
    match plan_for(action, &facts) {
        Plan::Skip(reason) => return finish(Outcome::Skipped(reason), ran),
        Plan::Nothing => return finish(Outcome::AlreadyCurrent, ran),
        Plan::Run => {}
    }
    let git = match Git::open_unattended(path) {
        Ok(git) => git,
        Err(error) => {
            let line = error_line(&error);
            return finish(
                outcome_of_error(&error),
                vec![Ran {
                    command: action.command().to_owned(),
                    result: line,
                }],
            );
        }
    };
    let fetching = matches!(
        action,
        BulkAction::Fetch | BulkAction::PullFastForward | BulkAction::Both
    );
    let mut switched: Option<(String, String)> = None;
    if matches!(action, BulkAction::SwitchToDefault | BulkAction::Both) && facts.off_default() {
        match git.switch_to_default() {
            Ok(to) => {
                ran.push(Ran {
                    command: format!("git checkout {to}"),
                    result: to.clone(),
                });
                let from = facts.branch.clone().unwrap_or_default();
                facts.branch = Some(to.clone());
                switched = Some((from, to));
            }
            Err(error) => {
                ran.push(Ran {
                    command: "git checkout <default>".to_owned(),
                    result: error_line(&error),
                });
                return finish(outcome_of_error(&error), ran);
            }
        }
    }
    if fetching {
        match git.fetch(None) {
            Ok(()) => ran.push(Ran {
                command: "git fetch --prune --all".to_owned(),
                result: "ok".to_owned(),
            }),
            Err(error) => {
                ran.push(Ran {
                    command: "git fetch --prune --all".to_owned(),
                    result: error_line(&error),
                });
                return finish(outcome_of_error(&error), ran);
            }
        }
    }
    let after = summary::read(path, stop);
    let behind_now = after.summary().map_or(0, |read| read.behind);
    let outcome = match action {
        BulkAction::Fetch => {
            if behind_now > before.behind {
                Outcome::Updated { behind: behind_now }
            } else {
                Outcome::AlreadyCurrent
            }
        }
        BulkAction::SwitchToDefault => match switched {
            Some((from, to)) => Outcome::Switched {
                from,
                to,
                behind: behind_now,
            },
            None => Outcome::AlreadyCurrent,
        },
        BulkAction::PullFastForward | BulkAction::Both => {
            let known = after
                .summary()
                .map_or(facts.clone(), |read| Facts::of(read, is_worktree));
            match after_fetch(&known) {
                Plan::Skip(reason) => Outcome::Skipped(reason),
                Plan::Nothing => {
                    switched.map_or(Outcome::AlreadyCurrent, |(from, to)| Outcome::Switched {
                        from,
                        to,
                        behind: 0,
                    })
                }
                Plan::Run => pull(&git, known.behind, switched, &mut ran),
            }
        }
    };
    let final_reading = if matches!(outcome, Outcome::Updated { .. } | Outcome::Switched { .. })
        && matches!(action, BulkAction::PullFastForward | BulkAction::Both)
    {
        Some(summary::read(path, stop))
    } else {
        Some(after)
    };
    Executed {
        outcome,
        ran,
        after: final_reading,
    }
}

fn pull(git: &Git, behind: u32, switched: Option<(String, String)>, ran: &mut Vec<Ran>) -> Outcome {
    let command = "git pull --ff-only".to_owned();
    match git.pull(Some(PullStrategy::FastForwardOnly)) {
        Ok(IntegrateOutcome::Completed) => {
            ran.push(Ran {
                command,
                result: format!("\u{2193}{behind}"),
            });
            match switched {
                Some((from, to)) => Outcome::Switched { from, to, behind },
                None => Outcome::Updated { behind },
            }
        }
        Ok(IntegrateOutcome::Conflicts(files)) => {
            ran.push(Ran {
                command,
                result: format!("{} files in conflict", files.len()),
            });
            Outcome::Failed {
                last_line: format!("{} files in conflict", files.len()),
            }
        }
        Err(error) => {
            ran.push(Ran {
                command,
                result: error_line(&error),
            });
            outcome_of_error(&error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Facts {
        Facts {
            is_worktree: false,
            changed: 0,
            branch: Some("main".to_owned()),
            default_branch: Some("main".to_owned()),
            ahead: 0,
            behind: 0,
        }
    }

    #[test]
    fn a_worktree_is_never_touched() {
        let worktree = Facts {
            is_worktree: true,
            ..facts()
        };
        for action in BulkAction::ALL {
            assert_eq!(
                plan_for(action, &worktree),
                Plan::Skip(SkipReason::Worktree)
            );
        }
    }

    #[test]
    fn local_changes_stop_everything_but_a_fetch() {
        let dirty = Facts {
            changed: 3,
            ..facts()
        };
        assert_eq!(plan_for(BulkAction::Fetch, &dirty), Plan::Run);
        for action in [
            BulkAction::SwitchToDefault,
            BulkAction::PullFastForward,
            BulkAction::Both,
        ] {
            assert_eq!(
                plan_for(action, &dirty),
                Plan::Skip(SkipReason::LocalChanges(3))
            );
        }
    }

    #[test]
    fn switching_only_happens_off_the_default_branch() {
        assert_eq!(
            plan_for(BulkAction::SwitchToDefault, &facts()),
            Plan::Nothing
        );
        let elsewhere = Facts {
            branch: Some("feature".to_owned()),
            ..facts()
        };
        assert_eq!(plan_for(BulkAction::SwitchToDefault, &elsewhere), Plan::Run);
        let detached = Facts {
            branch: None,
            ..facts()
        };
        assert_eq!(plan_for(BulkAction::SwitchToDefault, &detached), Plan::Run);
        let unknown_default = Facts {
            default_branch: None,
            branch: Some("feature".to_owned()),
            ..facts()
        };
        assert_eq!(
            plan_for(BulkAction::SwitchToDefault, &unknown_default),
            Plan::Nothing
        );
    }

    #[test]
    fn a_pull_and_a_fetch_run_on_a_clean_repo() {
        for action in [
            BulkAction::Fetch,
            BulkAction::PullFastForward,
            BulkAction::Both,
        ] {
            assert_eq!(plan_for(action, &facts()), Plan::Run);
        }
    }

    #[test]
    fn after_the_fetch_the_distance_decides() {
        assert_eq!(after_fetch(&facts()), Plan::Nothing);
        let behind = Facts {
            behind: 2,
            ..facts()
        };
        assert_eq!(after_fetch(&behind), Plan::Run);
        let ahead = Facts {
            ahead: 4,
            ..facts()
        };
        assert_eq!(after_fetch(&ahead), Plan::Nothing);
        let diverged = Facts {
            ahead: 1,
            behind: 2,
            ..facts()
        };
        assert_eq!(
            after_fetch(&diverged),
            Plan::Skip(SkipReason::Diverged { ahead: 1 })
        );
    }

    #[test]
    fn what_git_said_becomes_a_result() {
        assert_eq!(
            outcome_of_error(&ActionError::AuthFailed {
                remote: "origin".to_owned(),
                git_output: "denied".to_owned()
            }),
            Outcome::NoAccess {
                remote: "origin".to_owned()
            }
        );
        assert_eq!(
            outcome_of_error(&ActionError::Other {
                git_output: "fatal: bad\n\nhint: look\n".to_owned()
            }),
            Outcome::Failed {
                last_line: "hint: look".to_owned()
            }
        );
        assert!(matches!(
            outcome_of_error(&ActionError::NotFastForward {
                git_output: String::new()
            }),
            Outcome::Skipped(_)
        ));
    }

    #[test]
    fn a_result_counts_as_a_problem_unless_it_is_a_worktree() {
        assert!(Outcome::Failed {
            last_line: String::new()
        }
        .is_problem());
        assert!(Outcome::Skipped(SkipReason::LocalChanges(1)).is_problem());
        assert!(!Outcome::Skipped(SkipReason::Worktree).is_problem());
        assert!(!Outcome::Updated { behind: 1 }.is_problem());
        assert!(!Outcome::AlreadyCurrent.is_problem());
    }

    #[test]
    fn the_last_line_skips_blank_ones() {
        assert_eq!(last_line("a\n b \n\n"), "b");
        assert_eq!(last_line(""), "");
    }
}
