use crate::cli::Git;
use crate::integrate::IntegrateOutcome;
use crate::ActionError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StashEntry {
    /// The stash commit: the address that stays valid when the list shifts.
    pub sha: String,
    pub message: String,
}

const FIELD_SEPARATOR: char = '\u{1f}';

impl Git {
    pub fn stashes(&self) -> Result<Vec<StashEntry>, ActionError> {
        let format = format!("--format=%H{FIELD_SEPARATOR}%gs");
        let listing = self.text(&["stash", "list", &format])?;
        Ok(listing
            .lines()
            .filter_map(|line| {
                let (sha, message) = line.split_once(FIELD_SEPARATOR)?;
                Some(StashEntry {
                    sha: sha.to_string(),
                    message: message.to_string(),
                })
            })
            .collect())
    }

    /// Stashes the working changes (and the untracked files when asked). `None`
    /// when there was nothing to stash.
    pub fn stash_push(
        &self,
        message: &str,
        include_untracked: bool,
    ) -> Result<Option<StashEntry>, ActionError> {
        let before = self.stashes()?.first().map(|entry| entry.sha.clone());
        let mut args = vec!["stash", "push", "--message", message];
        if include_untracked {
            args.push("--include-untracked");
        }
        self.ok(&args)?;
        Ok(self
            .stashes()?
            .into_iter()
            .next()
            .filter(|entry| Some(&entry.sha) != before.as_ref()))
    }

    /// Applies the stash and keeps it. Conflicts come back as an outcome and the stash stays.
    pub fn stash_apply(&self, sha: &str) -> Result<IntegrateOutcome, ActionError> {
        self.stash_position(sha)?;
        let output = self.run(&["stash", "apply", sha], None)?;
        self.finished_integration(&output)
    }

    /// Applies the stash and drops it, unless the apply stopped on conflicts.
    pub fn stash_pop(&self, sha: &str) -> Result<IntegrateOutcome, ActionError> {
        let outcome = self.stash_apply(sha)?;
        if outcome == IntegrateOutcome::Completed {
            self.stash_drop(sha)?;
        }
        Ok(outcome)
    }

    pub fn stash_drop(&self, sha: &str) -> Result<(), ActionError> {
        let position = self.stash_position(sha)?;
        self.ok(&["stash", "drop", &format!("stash@{{{position}}}")])?;
        Ok(())
    }

    /// Where the stash is in the list right now.
    fn stash_position(&self, sha: &str) -> Result<usize, ActionError> {
        self.stashes()?
            .iter()
            .position(|entry| entry.sha == sha)
            .ok_or_else(|| ActionError::Other {
                git_output: format!("stash {sha} no longer exists"),
            })
    }
}
