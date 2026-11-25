use crate::cli::Git;
use crate::integrate::IntegrateOutcome;
use crate::refname::plain_ref;
use crate::ActionError;

/// How a pull on diverged branches joins them, for this one call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PullStrategy {
    Merge,
    Rebase,
    /// Moves the branch forward or does nothing: diverged branches are `NotFastForward`.
    FastForwardOnly,
}

impl PullStrategy {
    /// Both keys are always set: a `pull.rebase` or `pull.ff` in the user's
    /// config must not change what the chosen way means.
    fn settings(self) -> [&'static str; 2] {
        match self {
            Self::Merge => ["pull.rebase=false", "pull.ff=true"],
            Self::Rebase => ["pull.rebase=true", "pull.ff=true"],
            Self::FastForwardOnly => ["pull.rebase=false", "pull.ff=only"],
        }
    }
}

/// What a force push with lease would do, read before anything is sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForcePushPlan {
    pub remote: String,
    pub branch: String,
    /// The branch name on the remote.
    pub target: String,
    /// The commit the remote branch was last seen at; the push only goes
    /// through while the remote is still there.
    pub expected: String,
    /// Commits on the remote branch that this branch does not have.
    pub lost: u32,
    /// The titles of the newest of them, at most three.
    pub lost_titles: Vec<String>,
}

const LOST_TITLES_SHOWN: &str = "3";

impl Git {
    /// Fetches one remote, or all of them, and drops remote-tracking refs that are gone.
    pub fn fetch(&self, remote: Option<&str>) -> Result<(), ActionError> {
        match remote {
            Some(name) => self.talk(&["fetch", "--prune", "--", plain_ref(name)?], name)?,
            None => self.talk(&["fetch", "--prune", "--all"], "all remotes")?,
        };
        Ok(())
    }

    /// Pull the way the user's git config says (fast-forward, merge or rebase),
    /// or the way `strategy` says for this call; the config is never written.
    /// Conflicts come back as an outcome, not as an error.
    pub fn pull(&self, strategy: Option<PullStrategy>) -> Result<IntegrateOutcome, ActionError> {
        let remote = self
            .current_branch()
            .and_then(|branch| self.branch_setting(&branch, "remote"))
            .unwrap_or_else(|| "origin".to_string());
        let output = match strategy {
            Some(chosen) => {
                let [rebase, fast_forward] = chosen.settings();
                self.run(&["-c", rebase, "-c", fast_forward, "pull"], None)?
            }
            None => self.run(&["pull"], None)?,
        };
        if output.success {
            return Ok(IntegrateOutcome::Completed);
        }
        self.failed_integration(&output, &remote)
    }

    /// Pushes the checked-out branch.
    pub fn push(&self) -> Result<(), ActionError> {
        let branch = self.current_branch().ok_or_else(|| ActionError::Other {
            git_output: "HEAD is detached: there is no branch to push".to_string(),
        })?;
        self.push_branch(&branch, None)
    }

    /// Pushes a local branch, checked out or not, and sets its upstream when it has none.
    pub fn push_branch(&self, branch: &str, requested: Option<&str>) -> Result<(), ActionError> {
        let (configured, remote_name, target) = self.push_destination(branch, requested)?;
        let refspec = format!("refs/heads/{branch}:{target}");
        let mut args = vec!["push"];
        if configured.is_none() {
            args.push("--set-upstream");
        }
        args.push("--");
        args.push(&remote_name);
        args.push(&refspec);
        match self.talk(&args, &remote_name) {
            Ok(_) => Ok(()),
            Err(ActionError::Rejected { .. }) => {
                let tracking = format!(
                    "{branch}..refs/remotes/{remote_name}/{}",
                    target.trim_start_matches("refs/heads/")
                );
                let behind = self
                    .try_text(&["rev-list", "--count", &tracking, "--"])
                    .and_then(|count| count.parse().ok())
                    .unwrap_or(0);
                Err(ActionError::Rejected { behind })
            }
            Err(other) => Err(other),
        }
    }

    /// Whether the branch has an upstream, the remote to push to and the full ref name there.
    fn push_destination(
        &self,
        branch: &str,
        requested: Option<&str>,
    ) -> Result<(Option<String>, String, String), ActionError> {
        plain_ref(branch)?;
        let configured = self.branch_setting(branch, "remote");
        let remote_name = match requested.map(str::to_string).or_else(|| configured.clone()) {
            Some(found) => found,
            None => self.default_remote()?,
        };
        let target = self
            .branch_setting(branch, "merge")
            .unwrap_or_else(|| format!("refs/heads/{branch}"));
        Ok((configured, remote_name, target))
    }

    /// Looks at what a force push of the checked-out branch would replace.
    pub fn prepare_force_push(&self) -> Result<ForcePushPlan, ActionError> {
        let branch = self.current_branch().ok_or_else(|| ActionError::Other {
            git_output: "HEAD is detached: there is no branch to push".to_string(),
        })?;
        let (configured, remote, pushed_to) = self.push_destination(&branch, None)?;
        if configured.is_none() {
            return Err(ActionError::Other {
                git_output: format!("{branch} has no upstream: push it normally first"),
            });
        }
        let target = pushed_to.trim_start_matches("refs/heads/").to_string();
        let tracking = format!("refs/remotes/{remote}/{target}");
        let expected = self
            .try_text(&["rev-parse", "--verify", "-q", &tracking])
            .filter(|sha| !sha.is_empty())
            .ok_or_else(|| ActionError::Other {
                git_output: format!("{remote}/{target} is not known here: fetch first"),
            })?;
        let missing_here = format!("refs/heads/{branch}..{expected}");
        let lost = self
            .try_text(&["rev-list", "--count", &missing_here, "--"])
            .and_then(|count| count.parse().ok())
            .unwrap_or(0);
        let lost_titles = self
            .try_text(&[
                "log",
                "-n",
                LOST_TITLES_SHOWN,
                "--format=%s",
                &missing_here,
                "--",
            ])
            .map(|titles| titles.lines().map(str::to_string).collect())
            .unwrap_or_default();
        Ok(ForcePushPlan {
            remote,
            branch,
            target,
            expected,
            lost,
            lost_titles,
        })
    }

    /// Replaces the remote branch with the local one, but only while the remote
    /// is still at `plan.expected`; otherwise `LeaseRejected` and nothing changes.
    /// The expected commit is always spelled out: a bare lease checks the
    /// tracking ref, which a fetch in the background moves without anyone looking.
    pub fn force_push_with_lease(&self, plan: &ForcePushPlan) -> Result<(), ActionError> {
        plain_ref(&plan.remote)?;
        plain_ref(&plan.branch)?;
        let lease = format!(
            "--force-with-lease=refs/heads/{}:{}",
            plan.target, plan.expected
        );
        let refspec = format!("refs/heads/{}:refs/heads/{}", plan.branch, plan.target);
        self.talk(
            &["push", &lease, "--", &plan.remote, &refspec],
            &plan.remote,
        )?;
        Ok(())
    }

    /// Deletes a branch on the remote.
    pub fn delete_remote_branch(&self, remote: &str, branch: &str) -> Result<(), ActionError> {
        self.talk(
            &["push", "--delete", "--", plain_ref(remote)?, branch],
            remote,
        )?;
        Ok(())
    }

    pub(crate) fn branch_setting(&self, branch: &str, key: &str) -> Option<String> {
        self.try_text(&["config", "--get", &format!("branch.{branch}.{key}")])
            .filter(|value| !value.is_empty())
    }

    fn default_remote(&self) -> Result<String, ActionError> {
        let remotes = self.text(&["remote"])?;
        let mut names = remotes
            .lines()
            .map(str::trim)
            .filter(|name| !name.is_empty());
        let first = names.next();
        let origin = remotes
            .lines()
            .map(str::trim)
            .find(|name| *name == "origin");
        origin
            .or(first)
            .map(str::to_string)
            .ok_or_else(|| ActionError::Other {
                git_output: "this repository has no remote".to_string(),
            })
    }
}
