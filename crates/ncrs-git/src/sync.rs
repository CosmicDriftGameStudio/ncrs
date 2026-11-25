use crate::cli::Git;
use crate::integrate::IntegrateOutcome;
use crate::refname::plain_ref;
use crate::repo_access::parse_id;
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
        let mut args = vec!["fetch", "--prune"];
        if self.is_unattended() {
            args.push("--no-recurse-submodules");
        }
        match remote {
            Some(name) => {
                args.extend(["--", plain_ref(name)?]);
                self.talk(&args, name)?
            }
            None => {
                args.push("--all");
                self.talk(&args, "all remotes")?
            }
        };
        Ok(())
    }

    /// Removes the linked worktree at `path` with `git worktree remove`, without
    /// `--force`: git refuses a folder with local changes or a lock. Its branch stays.
    pub fn remove_worktree(&self, path: &std::path::Path) -> Result<(), ActionError> {
        self.ok(&["worktree", "remove", "--", &path.to_string_lossy()])
            .map(|_| ())
    }

    /// Whether the config of this repository (its own file, a worktree file,
    /// or what they include) makes git start a program during a fetch: a
    /// credential helper, an ssh or proxy command, an upload-pack, a remote
    /// helper URL. A downloaded repository can carry any of these. When git
    /// cannot tell (an old git without `--show-scope`), the answer is yes.
    pub fn repository_config_runs_commands(&self) -> bool {
        match self.run(&["config", "--show-scope", "-z", "--list"], None) {
            Ok(output) if output.success => config_runs_commands(&output.stdout_text()),
            Ok(_) | Err(_) => true,
        }
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

    /// Deletes a branch on the remote. With `expected_tip` the delete only goes
    /// through while the remote branch still has that tip (`--force-with-lease`);
    /// otherwise it fails with `LeaseRejected`.
    pub fn delete_remote_branch(
        &self,
        remote: &str,
        branch: &str,
        expected_tip: Option<&str>,
    ) -> Result<(), ActionError> {
        let remote = plain_ref(remote)?;
        let lease = match expected_tip {
            Some(tip) => {
                parse_id(tip).map_err(|_unreadable| ActionError::Other {
                    git_output: format!("{branch} has no scanned remote commit"),
                })?;
                Some(format!("--force-with-lease=refs/heads/{branch}:{tip}"))
            }
            None => None,
        };
        let mut args = vec!["push"];
        args.extend(lease.as_deref());
        args.extend(["--delete", "--", remote, branch]);
        self.talk(&args, remote)?;
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

/// Reads the `scope NUL name LF value NUL` entries of `git config --show-scope -z
/// --list` and looks at what the repository's own files set (`local`,
/// `worktree`; what they include shows up under their scope). Without `-z` a
/// name could hide behind an `=` or a line break of its own.
fn config_runs_commands(listing: &str) -> bool {
    let mut parts = listing.split('\0');
    while let (Some(scope), Some(setting)) = (parts.next(), parts.next()) {
        if scope != "local" && scope != "worktree" {
            continue;
        }
        let (name, value) = setting.split_once('\n').unwrap_or((setting, ""));
        if setting_runs_commands(&name.to_ascii_lowercase(), value) {
            return true;
        }
    }
    false
}

/// `name` is lowercase; the part between the first and last dot (a remote or
/// URL name) may hold anything, so only the ends are compared.
fn setting_runs_commands(name: &str, value: &str) -> bool {
    let last = name.rsplit('.').next().unwrap_or_default();
    let starts = |prefix: &str| name.starts_with(prefix);
    match name {
        "core.sshcommand" | "core.gitproxy" | "core.askpass" | "core.alternaterefscommand"
        | "gc.recentobjectshook" => true,
        _ if starts("credential.") => last == "helper",
        _ if starts("remote.") => {
            matches!(last, "uploadpack" | "receivepack" | "vcs")
                || (matches!(last, "url" | "pushurl") && value.contains("::"))
        }
        _ if starts("url.") => {
            matches!(last, "insteadof" | "pushinsteadof")
                && (name.contains("::") || value.contains("::"))
        }
        _ => false,
    }
}

#[cfg(test)]
mod config_tests {
    use super::config_runs_commands;

    fn runs(lines: &[&str]) -> bool {
        let listing: String = lines
            .iter()
            .map(|line| {
                let (scope, setting) = line.split_once('\t').unwrap();
                format!("{scope}\0{}\0", setting.replacen("==", "\n", 1))
            })
            .collect();
        config_runs_commands(&listing)
    }

    #[test]
    fn a_local_credential_helper_runs_a_program() {
        assert!(runs(&["local\tcredential.helper==!sh -c evil"]));
        assert!(runs(&["local\tcredential.https://x.test.helper==store"]));
        assert!(runs(&["worktree\tcore.sshcommand==ssh -o ProxyCommand=evil"]));
        assert!(runs(&["local\tremote.origin.uploadpack==evil"]));
        assert!(runs(&["local\tgc.recentobjectshook==evil"]));
    }

    #[test]
    fn an_equals_sign_in_the_name_hides_nothing() {
        assert!(runs(&["local\tcredential.https://a=b@x.test.helper==!sh -c evil"]));
        assert!(runs(&["local\tremote.a=b.uploadpack==evil"]));
    }

    #[test]
    fn a_command_transport_url_runs_a_program() {
        assert!(runs(&["local\tremote.origin.url==ext::sh -c evil"]));
        assert!(runs(&["local\turl.ext::sh -c evil.insteadof==https://x.test/"]));
        assert!(runs(&["local\turl.https://x.test/.insteadof==ext::sh evil"]));
    }

    #[test]
    fn what_an_include_sets_counts_under_the_scope_of_the_file_that_includes_it() {
        assert!(runs(&[
            "local\tinclude.path==../extra",
            "local\tcredential.helper==from-the-included-file"
        ]));
    }

    #[test]
    fn the_users_own_global_settings_do_not_count() {
        assert!(!runs(&[
            "global\tcredential.helper==osxkeychain",
            "system\tcore.sshcommand==ssh",
        ]));
    }

    #[test]
    fn an_ordinary_repository_is_clean() {
        assert!(!runs(&[
            "local\tcore.repositoryformatversion==0",
            "local\tremote.origin.url==https://x.test/repo.git",
            "local\tremote.origin.fetch==+refs/heads/*:refs/remotes/origin/*",
            "local\tbranch.main.remote==origin",
        ]));
    }
}
