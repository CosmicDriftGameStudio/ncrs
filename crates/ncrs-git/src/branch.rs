use crate::cli::Git;
use crate::undo::{UndoAction, UndoRecord, UndoSummary};
use crate::ActionError;

impl Git {
    /// Creates `name` at `from` (a branch, tag or sha), and switches to it when `checkout`.
    pub fn create_branch(&self, name: &str, from: &str, checkout: bool) -> Result<(), ActionError> {
        if checkout {
            self.ok(&["checkout", "-b", name, from])?;
        } else {
            self.ok(&["branch", name, from])?;
        }
        Ok(())
    }

    pub fn checkout(&self, branch: &str) -> Result<(), ActionError> {
        self.ok(&["checkout", branch])?;
        Ok(())
    }

    /// `remote_branch` is `origin/feature`: makes a local `feature` that tracks it.
    pub fn checkout_remote_branch(&self, remote_branch: &str) -> Result<(), ActionError> {
        self.ok(&["checkout", "--track", remote_branch])?;
        Ok(())
    }

    pub fn rename_branch(&self, old: &str, new: &str) -> Result<(), ActionError> {
        self.ok(&["branch", "-m", old, new])?;
        Ok(())
    }

    /// Deletes a local branch. Not merged and not `force`: `BranchNotMerged`.
    pub fn delete_branch(&self, name: &str, force: bool) -> Result<UndoRecord, ActionError> {
        let sha = self.text(&["rev-parse", "--verify", &format!("refs/heads/{name}")])?;
        let output = self.run(&["branch", if force { "-D" } else { "-d" }, name], None)?;
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
