//! What a short German or English sentence asks of Hermes. Fixed word lists,
//! no model: anything not recognised is `Intent::Unknown`.

use std::path::PathBuf;

mod intent;
mod reply;
mod shell;
#[cfg(test)]
mod tests;
mod words;

pub use intent::intent_of;
pub use reply::reply_of;
pub use words::{normalize_word, same_word};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    TidyFolder {
        folder: Option<FolderRef>,
    },
    LearnStructure {
        folder: Option<FolderRef>,
    },
    /// `words` is the rest of the sentence after the trigger word.
    Search {
        words: String,
    },
    Explain {
        words: String,
    },
    Undo,
    BatchMove {
        words: String,
    },
    ChangeOrder {
        kind_word: String,
        unit: TimeUnit,
    },
    /// Looks like a command for a shell; Hermes never runs it.
    ShellCommand,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderRef {
    Downloads,
    Documents,
    Desktop,
    /// As typed; a leading `~` is the chat's to expand.
    Path(PathBuf),
    /// A single capitalised word after the verb, only when no path was given.
    Named(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeUnit {
    Year,
    Month,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    Yes,
    No,
    Cancel,
}
