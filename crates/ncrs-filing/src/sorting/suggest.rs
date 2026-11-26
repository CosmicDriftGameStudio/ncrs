//! What the AI proposes for the rules of a target: a scheme for a whole
//! target, the rules a target with rules still lacks, and one rule for a file
//! no rule takes. Documents are grouped without a model, one model call per
//! group at most writes the pattern, and every candidate is tested by deciding
//! with it. Nothing is saved before `adopt`: a model never files and never
//! saves a rule of its own.

use std::path::PathBuf;

use ncrs_index::{DocumentFields, RuleSpec, TargetId};

/// Why there is no suggestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuggestError {
    Index(String),
    Stopped,
    /// The target is not configured any more.
    NoTarget,
    /// The source files into no target that exists.
    NoSource,
    /// The index does not know the file; suggestions never read a file.
    NotKnown,
    NoText,
    /// Nothing the file shows could be written into a path.
    NoPattern,
}

impl std::fmt::Display for SuggestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Index(detail) => f.write_str(detail),
            Self::Stopped => f.write_str("stopped"),
            Self::NoTarget => f.write_str("the target does not exist"),
            Self::NoSource => f.write_str("the source has no target"),
            Self::NotKnown => f.write_str("the index does not know the file"),
            Self::NoText => f.write_str("the file has no text"),
            Self::NoPattern => f.write_str("no path can be written from the file"),
        }
    }
}

impl std::error::Error for SuggestError {}

/// What the model did for a result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelNote {
    /// At least one pattern or rule came from the model.
    Used,
    /// There is no model: the result comes from the documents alone.
    NotAvailable,
    /// The model failed once and was not asked again; the reason is its own.
    Failed(String),
}

/// One proposed rule. Not stored anywhere.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggested {
    pub name: String,
    /// Unconfirmed, as `adopt` stores it before it confirms it.
    pub spec: RuleSpec,
    /// A path the rule gives one of the documents it is for.
    pub example: Option<String>,
    /// The documents the rule files.
    pub files: usize,
    /// The documents of the group the rule is for.
    pub group_documents: u32,
    /// The documents of other groups the rule takes too.
    pub elsewhere: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Suggestions {
    /// The documents with text that were looked at.
    pub files_read: usize,
    /// The sources of the target that were read.
    pub sources_read: usize,
    /// The rules the target had before.
    pub rules_existing: usize,
    /// Of `files_read`, the ones the suggested rules file.
    pub fit_suggested: usize,
    /// Of `files_read`, the ones neither the target's rules nor the suggested
    /// ones file.
    pub fit_none: usize,
    pub items: Vec<Suggested>,
    pub model: ModelNote,
}

/// One suggested rule for one file, and what else it would take.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleForFile {
    pub target: TargetId,
    pub rule: Suggested,
    /// What the index knows of the file.
    pub fields: DocumentFields,
    /// Other documents no rule takes yet that the rule would file.
    pub also: Vec<PathBuf>,
    pub model: ModelNote,
}
