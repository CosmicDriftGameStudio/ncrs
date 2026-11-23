use crate::cli::Git;
use crate::integrate::IntegrateOutcome;
use crate::refname::plain_ref;
use crate::ActionError;

/// How a pull on diverged branches joins them, for this one call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PullStrategy {
    Merge,
    Rebase,
}

impl PullStrategy {
    fn setting(self) -> &'static str {
        match self {
            Self::Merge => "pull.rebase=false",
            Self::Rebase => "pull.rebase=true",
        }
    }
}

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
            Some(chosen) => self.run(&["-c", chosen.setting(), "pull"], None)?,
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
        plain_ref(branch)?;
        let configured = self.branch_setting(branch, "remote");
        let remote_name = match requested.map(str::to_string).or_else(|| configured.clone()) {
            Some(found) => found,
            None => self.default_remote()?,
        };
        let target = self
            .branch_setting(branch, "merge")
            .unwrap_or_else(|| format!("refs/heads/{branch}"));
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

    /// Deletes a branch on the remote.
    pub fn delete_remote_branch(&self, remote: &str, branch: &str) -> Result<(), ActionError> {
        self.talk(
            &["push", "--delete", "--", plain_ref(remote)?, branch],
            remote,
        )?;
        Ok(())
    }

    fn branch_setting(&self, branch: &str, key: &str) -> Option<String> {
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
