use crate::cli::Git;
use crate::integrate::IntegrateOutcome;
use crate::refname::plain_ref;
use crate::undo::{UndoAction, UndoRecord, UndoSummary};
use crate::ActionError;

/// How far a reset reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetMode {
    /// Moves the branch only; index and files stay.
    Soft,
    /// Moves the branch and resets the index; files stay.
    Mixed,
    /// Moves the branch, resets index and files; changes to tracked files are lost.
    Hard,
}

impl ResetMode {
    fn flag(self) -> &'static str {
        match self {
            Self::Soft => "--soft",
            Self::Mixed => "--mixed",
            Self::Hard => "--hard",
        }
    }
}

/// What a hard reset to a commit would throw away.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResetLoss {
    /// Commits of the branch that are not reachable from the target.
    pub commits: u32,
    /// Tracked files with changes, staged or not. Untracked files stay.
    pub changed_files: u32,
}

/// A pick or revert: it either went through (with its undo) or stopped on conflicts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickOutcome {
    pub outcome: IntegrateOutcome,
    /// `None` when the pick stopped on conflicts, or there was no commit to go back from.
    pub undo: Option<UndoRecord>,
}

impl Git {
    /// Checks the commit out with HEAD detached.
    pub fn checkout_detached(&self, sha: &str) -> Result<(), ActionError> {
        self.ok(&["checkout", "--detach", plain_ref(sha)?, "--"])?;
        Ok(())
    }

    /// A lightweight tag on `sha`. Undo deletes it again.
    pub fn create_tag(&self, name: &str, sha: &str) -> Result<UndoRecord, ActionError> {
        self.check_tag_name(name)?;
        let commit = self.commit_of(sha)?;
        self.ok(&["tag", "--", name, &commit])?;
        Ok(UndoRecord {
            summary: UndoSummary::CreateTag {
                name: name.to_string(),
            },
            description: format!("Create tag {name}"),
            action: UndoAction::DeleteTag {
                name: name.to_string(),
                sha: commit,
            },
        })
    }

    /// Applies the changes of `sha` on top of the checked-out branch as a new
    /// commit. Merge commits are refused.
    pub fn cherry_pick(&self, sha: &str) -> Result<PickOutcome, ActionError> {
        let commit = self.single_parent_commit(sha)?;
        self.pick(&["cherry-pick", "--", &commit], &commit, |title| {
            (
                UndoSummary::CherryPick {
                    title: title.to_string(),
                },
                format!("Cherry-pick \"{title}\""),
            )
        })
    }

    /// A new commit that takes the changes of `sha` back. Merge commits are refused.
    pub fn revert(&self, sha: &str) -> Result<PickOutcome, ActionError> {
        let commit = self.single_parent_commit(sha)?;
        self.pick(&["revert", "--no-edit", "--", &commit], &commit, |title| {
            (
                UndoSummary::Revert {
                    title: title.to_string(),
                },
                format!("Revert \"{title}\""),
            )
        })
    }

    /// Moves the checked-out branch to `sha`. The undo brings back the old
    /// position, the index and, after a hard reset, the changed files.
    pub fn reset(&self, mode: ResetMode, sha: &str) -> Result<UndoRecord, ActionError> {
        let target = self.commit_of(sha)?;
        let previous_head = self.head_sha().ok_or_else(|| ActionError::Other {
            git_output: "there is no commit to reset from".to_string(),
        })?;
        let branch = self.current_branch().unwrap_or_else(|| "HEAD".to_string());
        let index_before = self.index_tree();
        let mut saved = if mode == ResetMode::Hard {
            self.save_changed_tracked()?
        } else {
            Vec::new()
        };
        self.ok(&["reset", mode.flag(), &target, "--"])?;
        self.mark_discarded(&mut saved);
        let short: String = target.chars().take(7).collect();
        Ok(UndoRecord {
            summary: UndoSummary::Reset {
                branch: branch.clone(),
                sha: short.clone(),
            },
            description: format!("Reset {branch} to {short}"),
            action: UndoAction::ResetBranch {
                previous_head,
                new_head: self.head_sha().unwrap_or(target),
                index_before,
                index_after: self.index_tree(),
                saved,
                hard: mode == ResetMode::Hard,
            },
        })
    }

    /// What `reset(Hard, sha)` would lose, for the question before it.
    pub fn reset_loss(&self, sha: &str) -> Result<ResetLoss, ActionError> {
        let target = self.commit_of(sha)?;
        let range = format!("{target}..HEAD");
        let commits = self
            .text(&["rev-list", "--count", &range, "--"])?
            .parse()
            .unwrap_or(0);
        let changed_files = self
            .porcelain()?
            .iter()
            .filter(|entry| !entry.is_untracked())
            .count();
        Ok(ResetLoss {
            commits,
            changed_files: u32::try_from(changed_files).unwrap_or(u32::MAX),
        })
    }

    fn commit_of(&self, sha: &str) -> Result<String, ActionError> {
        let spec = format!("{}^{{commit}}", plain_ref(sha)?);
        self.text(&["rev-parse", "--verify", "-q", &spec])
    }

    fn single_parent_commit(&self, sha: &str) -> Result<String, ActionError> {
        let commit = self.commit_of(sha)?;
        let second_parent = format!("{commit}^2");
        if self
            .try_text(&["rev-parse", "--verify", "-q", &second_parent])
            .is_some()
        {
            return Err(ActionError::Other {
                git_output: "a merge commit has no single change to pick".to_string(),
            });
        }
        Ok(commit)
    }

    fn pick(
        &self,
        args: &[&str],
        commit: &str,
        describe: impl FnOnce(&str) -> (UndoSummary, String),
    ) -> Result<PickOutcome, ActionError> {
        let previous = self.head_sha();
        let title = self
            .try_text(&["log", "-n", "1", "--format=%s", commit, "--"])
            .unwrap_or_default();
        let output = self.run(args, None)?;
        let outcome = self.finished_integration(&output)?;
        let undo = match (&outcome, previous, self.head_sha()) {
            (IntegrateOutcome::Completed, Some(previous_head), Some(new_head)) => {
                let (summary, description) = describe(&title);
                Some(UndoRecord {
                    summary,
                    description,
                    action: UndoAction::ResetPick {
                        new_head,
                        previous_head,
                    },
                })
            }
            _ => None,
        };
        Ok(PickOutcome { outcome, undo })
    }
}
