//! State of the modal prompt.
//!
//! Lives next to `App` rather than in `ui/`, because it is state, not a view:
//! `App` owns it and only `App::update` changes it. The matching `view` function
//! is in `ui::dialog`.
//!
//! Not yet in the roadmap's T10 (config): the prompt remembers the name the
//! user typed while the operation runs, so a failure can be shown next to the
//! text that caused it.
use std::path::PathBuf;

/// Which operation the prompt is collecting input for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    /// F7 — ask for a directory name to create in the active panel.
    CreateDir,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prompt {
    CreateDir {
        /// The directory the new one goes into, and the panel to reload.
        parent: PathBuf,
        /// What the user typed. Kept across a failed attempt so the error sits
        /// next to the text that caused it.
        name: String,
        /// Set while the operation runs.
        busy: bool,
        /// Why the last attempt failed, in the active language.
        error: Option<String>,
    },
}

impl Prompt {
    pub fn create_dir(parent: PathBuf) -> Self {
        Self::CreateDir {
            parent,
            name: String::new(),
            busy: false,
            error: None,
        }
    }

    /// Replaces the typed name and clears any error: a new keystroke means the
    /// previous complaint no longer applies.
    pub fn set_name(&mut self, name: String) {
        match self {
            Prompt::CreateDir {
                name: current,
                error,
                ..
            } => {
                *current = name;
                *error = None;
            }
        }
    }

    /// The text the user typed.
    pub fn name(&self) -> &str {
        match self {
            Prompt::CreateDir { name, .. } => name,
        }
    }

    /// Marks the operation as running or finished.
    pub fn set_busy(&mut self, busy: bool) {
        match self {
            Prompt::CreateDir { busy: current, .. } => *current = busy,
        }
    }

    pub fn busy(&self) -> bool {
        match self {
            Prompt::CreateDir { busy, .. } => *busy,
        }
    }

    /// Shows a validation failure, in the language the UI is in.
    pub fn set_validation_error(&mut self, reason: &'static str) {
        match self {
            Prompt::CreateDir { error, .. } => *error = Some(reason.to_string()),
        }
    }

    /// Shows a failure from the filesystem.
    pub fn set_error(&mut self, message: String) {
        match self {
            Prompt::CreateDir { error, .. } => *error = Some(message),
        }
    }

    pub fn error(&self) -> Option<&str> {
        match self {
            Prompt::CreateDir { error, .. } => error.as_deref(),
        }
    }

    /// Whether the typed name could be a directory name. Empty, `.` and `..`
    /// are rejected before the filesystem is touched, so the error appears while
    /// the dialog is still open instead of after a round trip.
    pub fn validate(&self) -> Result<(), &'static str> {
        let name = self.name().trim();
        if name.is_empty() {
            return Err("empty_name");
        }
        if name == "." || name == ".." {
            return Err("reserved_name");
        }
        if name.contains('/') || name.contains('\\') {
            return Err("separator_not_allowed");
        }
        Ok(())
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    fn prompt_with(name: &str) -> Prompt {
        Prompt::CreateDir {
            parent: PathBuf::from("/tmp"),
            name: name.to_string(),
            busy: false,
            error: None,
        }
    }

    #[test]
    fn an_ordinary_name_is_accepted() {
        assert_eq!(prompt_with("Projekte").validate(), Ok(()));
        // A name with spaces is normal on every platform here.
        assert_eq!(prompt_with("Meine Dateien").validate(), Ok(()));
        // Non-ASCII must work; this is a file manager, not a shell.
        assert_eq!(prompt_with("Übersicht").validate(), Ok(()));
    }

    /// The three rejections that a user can type but a filesystem would answer
    /// late and less clearly.
    #[test]
    fn names_that_cannot_be_created_are_rejected() {
        assert_eq!(prompt_with("").validate(), Err("empty_name"));
        assert_eq!(prompt_with("   ").validate(), Err("empty_name"));
        assert_eq!(prompt_with(".").validate(), Err("reserved_name"));
        assert_eq!(prompt_with("..").validate(), Err("reserved_name"));
        assert_eq!(prompt_with("a/b").validate(), Err("separator_not_allowed"));
        assert_eq!(prompt_with(r"a\b").validate(), Err("separator_not_allowed"));
    }

    /// A rejected name must never reach the filesystem, because `..` would
    /// create a directory above the active panel. The name and the parent are
    /// joined by the caller, so the validation is the only thing standing
    /// between a keystroke and the parent directory.
    #[test]
    fn a_traversal_name_is_rejected_before_it_can_be_joined() {
        assert!(prompt_with("..").validate().is_err());
        assert!(prompt_with("a/b").validate().is_err());
    }

    /// Whitespace around a name is easy to type and should not create a
    /// directory called " x".
    #[test]
    fn surrounding_whitespace_is_accepted_and_left_to_the_target() {
        let p = prompt_with("  spaced  ");
        assert_eq!(p.validate(), Ok(()));
        assert_eq!(p.name(), "  spaced  ");
    }
}
