//! Filing rules in both directions: a description becomes a rule, and the
//! paths of filed documents become the pattern that wrote them. The model
//! writes the real `RuleSpec` and pattern syntax, in a grammar built from the
//! constants of the rule language; whether the answer is right is decided by
//! rendering it, never by comparing text.

mod annotate;
mod fixtures;
mod grammar;
mod levels;
mod parse;
mod prompt;
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

#[cfg(feature = "llama")]
pub use annotate::annotated_paths;
#[cfg(feature = "llama")]
pub use levels::{Level, Levels, Part};
#[cfg(feature = "llama")]
pub use parse::{parse_generate, parse_levels};
#[cfg(feature = "llama")]
pub use prompt::{prefix_text, ContextDocument, END_OF_TURN};
#[cfg(feature = "llama")]
pub use session::RuleRequest;
#[cfg(feature = "llama")]
pub use verify::{pattern_mismatches, Mismatch};
