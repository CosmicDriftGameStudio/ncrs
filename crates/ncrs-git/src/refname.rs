use crate::cli::Git;
use crate::ActionError;

fn invalid(name: &str) -> ActionError {
    ActionError::InvalidName {
        name: name.to_string(),
    }
}

/// A branch, tag, remote or commit named by the user or the view must never
/// read as an option: git would run it as one wherever `--` is not accepted.
pub fn plain_ref(name: &str) -> Result<&str, ActionError> {
    if name.is_empty() || name.starts_with('-') {
        return Err(invalid(name));
    }
    Ok(name)
}

impl Git {
    /// A name for a branch that is about to be made, as git itself judges it.
    pub fn check_branch_name(&self, name: &str) -> Result<(), ActionError> {
        plain_ref(name)?;
        if name.contains("@{") {
            return Err(invalid(name));
        }
        let output = self.run(&["check-ref-format", "--branch", name], None)?;
        if output.success {
            Ok(())
        } else {
            Err(invalid(name))
        }
    }
}
