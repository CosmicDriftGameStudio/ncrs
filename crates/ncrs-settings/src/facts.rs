//! What the Index and Hermes pages show that the app knows and the config file
//! does not. The app fills it when something changes, not on every frame.

use std::collections::BTreeMap;
use std::path::PathBuf;

pub use ncrs_core::facts::{AreaFacts, LoadedFacts, ReadFolderCoverage, RunDay, RunFacts};

/// A model the user can load and remove again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelKind {
    Ocr,
    Ai,
}

/// How the AI stands, as the Index page words it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum AiState {
    #[default]
    Off,
    Missing,
    Downloading,
    Ready,
    OwnServer {
        model: String,
        /// Why the last request to the server failed, worded.
        failure: Option<String>,
    },
    /// An own server on another computer that was not agreed to yet.
    NotAllowed,
    Failed,
    NextStart,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentsFacts {
    pub documents: u64,
    /// Documents the AI has read and sorted.
    pub understood: u64,
    /// Documents with a text are left for the AI and the index is not paused.
    pub work_left: bool,
    pub paused: bool,
    /// Files the index could not read.
    pub unreadable: usize,
    /// Locations whose folder cannot be reached right now.
    pub disconnected: Vec<PathBuf>,
    /// `None` while the folders are being looked at.
    pub areas: Option<Vec<AreaFacts>>,
    /// Newest first.
    pub runs: Vec<RunFacts>,
    /// How many documents each read folder holds; empty without an index.
    pub folder_files: BTreeMap<PathBuf, usize>,
    pub rules: usize,
    pub ai: AiState,
    /// What the model in use is, worded: file name, memory and what it
    /// computes on, or why it failed. `None` while there is nothing to add to
    /// the state.
    pub ai_detail: Option<String>,
    /// The system brings a recognizer for scans and photos (macOS Vision).
    pub system_reads_scans: bool,
    /// Days after which unused Hermes chats are deleted; `None` never.
    pub chat_keep_days: Option<u32>,
}

/// What the Favourites page shows: the app keeps the list, the page reports
/// edits as actions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FavouritesFacts {
    pub folders: Vec<PathBuf>,
    pub bar_shown: bool,
    /// The first and the last key of the five that open a favourite, when
    /// they are one run of keys; `None` when they are not.
    pub open_keys: Option<(String, String)>,
    /// The key that adds the current folder, when one is bound.
    pub add_key: Option<String>,
}
