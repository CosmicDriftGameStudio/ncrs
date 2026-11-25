use std::fmt;

use crate::GitError;

/// Why a write action did not happen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionError {
    /// No `git` on this machine: the view hides the actions.
    GitNotFound,
    /// The App Store build does not run write actions.
    DisabledInSandbox,
    /// The remote has commits this branch lacks (non-fast-forward). `behind` is
    /// counted against the local remote-tracking ref, so 0 when it is stale or missing.
    Rejected {
        behind: u32,
    },
    AuthFailed {
        remote: String,
        git_output: String,
    },
    NetworkFailed {
        git_output: String,
    },
    /// `branch -d` refused: offer a forced delete.
    BranchNotMerged {
        branch: String,
    },
    NothingToCommit,
    /// A branch or ref name git does not take: empty, starting with "-" or not a valid ref name.
    InvalidName {
        name: String,
    },
    /// Pull on diverged branches with no `pull.rebase` / `pull.ff` in the user's config.
    NeedsPullStrategy {
        git_output: String,
    },
    /// `pull --ff-only` on branches that have diverged: nothing was changed.
    NotFastForward {
        git_output: String,
    },
    /// A force push with lease found the remote branch somewhere else than
    /// the caller last saw it: nothing was overwritten.
    LeaseRejected,
    /// Files are still in conflict, or blocks inside a file.
    UnresolvedConflicts {
        count: usize,
    },
    /// The undo would overwrite or lose something newer.
    UndoNotSafe(String),
    Io(String),
    Read(GitError),
    Other {
        git_output: String,
    },
}

impl From<GitError> for ActionError {
    fn from(cause: GitError) -> Self {
        Self::Read(cause)
    }
}

impl fmt::Display for ActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GitNotFound => f.write_str("git is not installed"),
            Self::DisabledInSandbox => f.write_str("git actions are not available in this build"),
            Self::Rejected { behind } => {
                write!(f, "rejected: the remote is {behind} commit(s) ahead")
            }
            Self::AuthFailed { remote, git_output } => {
                write!(f, "authentication to {remote} failed: {git_output}")
            }
            Self::NetworkFailed { git_output } => write!(f, "network failure: {git_output}"),
            Self::BranchNotMerged { branch } => write!(f, "branch {branch} is not merged"),
            Self::NothingToCommit => f.write_str("nothing to commit"),
            Self::InvalidName { name } => write!(f, "\"{name}\" is not a valid name"),
            Self::NeedsPullStrategy { git_output } => {
                write!(f, "pull needs a strategy: {git_output}")
            }
            Self::NotFastForward { git_output } => {
                write!(f, "not a fast-forward: {git_output}")
            }
            Self::LeaseRejected => f.write_str("the remote branch changed since it was last seen"),
            Self::UnresolvedConflicts { count } => write!(f, "{count} conflict(s) unresolved"),
            Self::UndoNotSafe(reason) => write!(f, "undo not safe: {reason}"),
            Self::Io(cause) => write!(f, "io: {cause}"),
            Self::Read(cause) => cause.fmt(f),
            Self::Other { git_output } => write!(f, "git: {git_output}"),
        }
    }
}

impl std::error::Error for ActionError {}

const AUTH_MARKERS: &[&str] = &[
    "Authentication failed",
    "could not read Username",
    "could not read Password",
    "Permission denied (publickey",
    "Permission denied, please try again",
    "Host key verification failed",
    "terminal prompts disabled",
    "HTTP Basic: Access denied",
    "returned error: 401",
    "returned error: 403",
    "Invalid username or password",
    "Support for password authentication was removed",
];

const NETWORK_MARKERS: &[&str] = &[
    "Could not resolve host",
    "Could not resolve hostname",
    "Temporary failure in name resolution",
    "Connection timed out",
    "Operation timed out",
    "Connection refused",
    "Connection reset",
    "Network is unreachable",
    "No route to host",
    "Failed to connect to",
    "unable to access",
    "RPC failed",
    "early EOF",
];

/// Sorts what git printed (with `LC_ALL=C`) into a typed failure. Order matters:
/// a rejected push also says "failed to push", and an auth failure ends with
/// "Could not read from remote repository", so network comes last.
pub fn classify_sync_failure(git_output: &str, remote: &str) -> ActionError {
    if git_output.contains("(stale info)") {
        return ActionError::LeaseRejected;
    }
    let rejected = git_output.contains("[rejected]")
        && (git_output.contains("non-fast-forward") || git_output.contains("fetch first"))
        || git_output.contains("Updates were rejected because");
    if rejected {
        return ActionError::Rejected { behind: 0 };
    }
    if AUTH_MARKERS
        .iter()
        .any(|marker| git_output.contains(marker))
    {
        return ActionError::AuthFailed {
            remote: remote.to_string(),
            git_output: git_output.to_string(),
        };
    }
    if git_output.contains("Not possible to fast-forward") {
        return ActionError::NotFastForward {
            git_output: git_output.to_string(),
        };
    }
    if git_output.contains("Need to specify how to reconcile divergent branches") {
        return ActionError::NeedsPullStrategy {
            git_output: git_output.to_string(),
        };
    }
    if NETWORK_MARKERS
        .iter()
        .any(|marker| git_output.contains(marker))
    {
        return ActionError::NetworkFailed {
            git_output: git_output.to_string(),
        };
    }
    ActionError::Other {
        git_output: git_output.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_fast_forward_push_is_rejected() {
        let output = "To /srv/origin.git\n ! [rejected]        main -> main (non-fast-forward)\nerror: failed to push some refs to '/srv/origin.git'\nhint: Updates were rejected because the tip of your current branch is behind";
        assert_eq!(
            classify_sync_failure(output, "origin"),
            ActionError::Rejected { behind: 0 }
        );
    }

    #[test]
    fn fetch_first_is_rejected() {
        let output =
            " ! [rejected]        main -> main (fetch first)\nerror: failed to push some refs";
        assert!(matches!(
            classify_sync_failure(output, "origin"),
            ActionError::Rejected { .. }
        ));
    }

    #[test]
    fn ssh_key_refusal_is_auth() {
        let output = "git@github.com: Permission denied (publickey).\nfatal: Could not read from remote repository.\n\nPlease make sure you have the correct access rights\nand the repository exists.";
        assert!(matches!(
            classify_sync_failure(output, "origin"),
            ActionError::AuthFailed { remote, .. } if remote == "origin"
        ));
    }

    #[test]
    fn https_without_credentials_is_auth() {
        for output in [
            "fatal: could not read Username for 'https://github.com': terminal prompts disabled",
            "remote: Invalid username or password.\nfatal: Authentication failed for 'https://github.com/a/b.git/'",
            "fatal: unable to access 'https://x/y.git/': The requested URL returned error: 403",
        ] {
            assert!(
                matches!(
                    classify_sync_failure(output, "up"),
                    ActionError::AuthFailed { .. }
                ),
                "{output}"
            );
        }
    }

    #[test]
    fn unreachable_host_is_network() {
        for output in [
            "fatal: unable to access 'https://github.com/a/b.git/': Could not resolve host: github.com",
            "ssh: connect to host github.com port 22: Operation timed out\nfatal: Could not read from remote repository.",
            "ssh: Could not resolve hostname github.com: Temporary failure in name resolution",
            "fatal: unable to access 'https://h/r.git/': Failed to connect to h port 443: Connection refused",
        ] {
            assert!(
                matches!(
                    classify_sync_failure(output, "origin"),
                    ActionError::NetworkFailed { .. }
                ),
                "{output}"
            );
        }
    }

    #[test]
    fn divergent_pull_needs_a_strategy() {
        let output = "hint: You have divergent branches and need to specify how to reconcile them.\nfatal: Need to specify how to reconcile divergent branches.";
        assert!(matches!(
            classify_sync_failure(output, "origin"),
            ActionError::NeedsPullStrategy { .. }
        ));
    }

    #[test]
    fn a_stale_lease_is_not_a_plain_rejection() {
        let output = "To /srv/origin.git\n ! [rejected]        main -> main (stale info)\nerror: failed to push some refs";
        assert_eq!(
            classify_sync_failure(output, "origin"),
            ActionError::LeaseRejected
        );
    }

    #[test]
    fn a_pull_that_cannot_fast_forward_is_typed() {
        let output = "fatal: Not possible to fast-forward, aborting.";
        assert!(matches!(
            classify_sync_failure(output, "origin"),
            ActionError::NotFastForward { .. }
        ));
    }

    #[test]
    fn anything_else_keeps_the_output() {
        let output = "ERROR: Repository not found.\nfatal: Could-not-do-it";
        assert_eq!(
            classify_sync_failure(output, "origin"),
            ActionError::Other {
                git_output: output.to_string()
            }
        );
    }
}
