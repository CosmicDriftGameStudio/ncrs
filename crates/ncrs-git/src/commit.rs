use crate::cli::Git;
use crate::undo::{UndoAction, UndoRecord, UndoSummary};
use crate::ActionError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitMessage {
    pub title: String,
    pub body: String,
}

impl CommitMessage {
    fn text(&self) -> String {
        if self.body.trim().is_empty() {
            format!("{}\n", self.title.trim())
        } else {
            format!("{}\n\n{}\n", self.title.trim(), self.body.trim())
        }
    }
}

impl Git {
    /// Commits what is staged. `None` as the undo for a first commit (no parent to go back to).
    pub fn commit(&self, message: &CommitMessage) -> Result<Option<UndoRecord>, ActionError> {
        let previous = self.head_sha();
        self.commit_with(&["commit", "-F", "-"], message)?;
        let head_now = self.head_sha();
        Ok(previous
            .zip(head_now)
            .map(|(previous_head, new_head)| UndoRecord {
                summary: UndoSummary::Commit {
                    title: message.title.clone(),
                },
                description: format!("Commit \"{}\"", message.title.trim()),
                action: UndoAction::ResetCommit {
                    new_head,
                    previous_head,
                },
            }))
    }

    /// Rewrites the last commit with what is staged, and with `message` when given.
    pub fn amend(&self, message: Option<&CommitMessage>) -> Result<(), ActionError> {
        match message {
            Some(new_message) => self.commit_with(&["commit", "--amend", "-F", "-"], new_message),
            None => {
                let output = self.run(&["commit", "--amend", "--no-edit"], None)?;
                self.commit_result(&output)
            }
        }
    }

    fn commit_with(&self, args: &[&str], message: &CommitMessage) -> Result<(), ActionError> {
        let output = self.run(args, Some(message.text().as_bytes()))?;
        self.commit_result(&output)
    }

    fn commit_result(&self, output: &crate::cli::Output) -> Result<(), ActionError> {
        if output.success {
            return Ok(());
        }
        let text = output.combined();
        if text.contains("nothing to commit") || text.contains("nothing added to commit") {
            Err(ActionError::NothingToCommit)
        } else {
            Err(ActionError::Other { git_output: text })
        }
    }
}
