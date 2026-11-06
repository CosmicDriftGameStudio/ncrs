//! Filing rules in both directions: a description becomes a rule, and the
//! paths of filed documents become the pattern that wrote them. The model
//! writes the real `RuleSpec` and pattern syntax, in a grammar built from the
//! constants of the rule language; whether the answer is right is decided by
//! rendering it, never by comparing text.

mod annotate;
#[cfg(test)]
mod fixtures;
mod grammar;
mod levels;
mod parse;
mod prompt;
mod revise;
mod session;
mod verify;
mod vocabulary;

#[cfg(test)]
mod cache;
#[cfg(test)]
mod holdout;
#[cfg(all(test, feature = "llama"))]
mod live;
#[cfg(test)]
mod matrix;
#[cfg(test)]
mod oracle;

pub use annotate::{annotated_paths, folder_depth};
pub use levels::{Level, Levels, Part};
pub use parse::{parse_generate, parse_levels};
pub use prompt::{prefix_text, ContextDocument, END_OF_TURN};
pub use revise::revise_question;
pub use session::RuleRequest;
pub use verify::{
    file_names_share_structure, pattern_mismatches, repair_note, repair_note_for, Mismatch,
};
pub use vocabulary::condition_meaning;
