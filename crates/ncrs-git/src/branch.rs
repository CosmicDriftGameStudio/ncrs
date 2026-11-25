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
        // A partial clone fetches what the checkout lacks.
        self.refuse_commands_of_the_repository()?;
        self.ok(&["checkout", plain_ref(branch)?, "--"])?;
        Ok(())
    }

    /// `remote_branch` is `origin/feature`: makes a local `feature` that tracks it.
    pub fn checkout_remote_branch(&self, remote_branch: &str) -> Result<(), ActionError> {
        self.refuse_commands_of_the_repository()?;
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

    /// Switches to the default branch (what `origin/HEAD` names, else main or master),
    /// making a local branch that tracks origin first when there is none yet.
    /// Returns its name.
    pub fn switch_to_default(&self) -> Result<String, ActionError> {
        let repository =
            crate::repo_access::open(self.root()).map_err(|cause| ActionError::Other {
                git_output: cause.to_string(),
            })?;
        let found =
            crate::repo_access::default_branch(&repository).ok_or_else(|| ActionError::Other {
                git_output: "there is no default branch here".to_string(),
            })?;
        if found.local_tip.is_some() {
            self.checkout(&found.name)?;
        } else {
            self.checkout_remote_branch(&format!("origin/{}", found.name))?;
        }
        Ok(found.name)
    }
}
