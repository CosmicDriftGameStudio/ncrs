use crate::cli::Git;
use crate::refname::plain_ref;
use crate::undo::{UndoAction, UndoRecord, UndoSummary};
use crate::ActionError;

impl Git {
    /// Creates `name` at `from` (a branch, tag or sha), and switches to it when `checkout`.
    pub fn create_branch(&self, name: &str, from: &str, checkout: bool) -> Result<(), ActionError> {
        self.check_branch_name(name)?;
        let start = plain_ref(from)?;
        if checkout {
            self.ok(&["checkout", "-b", name, start])?;
        } else {
            self.ok(&["branch", "--", name, start])?;
        }
        Ok(())
    }

    pub fn checkout(&self, branch: &str) -> Result<(), ActionError> {
        self.ok(&["checkout", plain_ref(branch)?, "--"])?;
        Ok(())
    }

    /// `remote_branch` is `origin/feature`: makes a local `feature` that tracks it.
    pub fn checkout_remote_branch(&self, remote_branch: &str) -> Result<(), ActionError> {
        self.ok(&["checkout", "--track", plain_ref(remote_branch)?])?;
        Ok(())
    }

    pub fn rename_branch(&self, old: &str, new: &str) -> Result<(), ActionError> {
        self.check_branch_name(new)?;
        self.ok(&["branch", "-m", "--", plain_ref(old)?, new])?;
        Ok(())
    }

    /// Deletes a local branch. Not merged and not `force`: `BranchNotMerged`.
    pub fn delete_branch(&self, name: &str, force: bool) -> Result<UndoRecord, ActionError> {
        plain_ref(name)?;
        let sha = self.text(&["rev-parse", "--verify", &format!("refs/heads/{name}")])?;
        let output = self.run(
            &["branch", if force { "-D" } else { "-d" }, "--", name],
            None,
        )?;
        if !output.success {
            let text = output.combined();
            return Err(if text.contains("not fully merged") {
                ActionError::BranchNotMerged {
                    branch: name.to_string(),
                }
            } else {
                ActionError::Other { git_output: text }
            });
        }
        Ok(UndoRecord {
            summary: UndoSummary::DeleteBranch {
                name: name.to_string(),
            },
            description: format!("Delete branch {name}"),
            action: UndoAction::RecreateBranch {
                name: name.to_string(),
                sha,
            },
        })
    }
}
