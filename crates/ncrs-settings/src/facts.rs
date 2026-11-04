//! What the Index and Hermes pages show that the app knows and the config file
//! does not. The app fills it when something changes, not on every frame.

use std::path::PathBuf;

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
    },
    Failed,
    NextStart,
}

/// One area of the order Hermes learned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AreaFacts {
    pub name: String,
    pub files: usize,
    /// The read folder the area lies in.
    pub read_folder: PathBuf,
    pub root: PathBuf,
    /// How the area orders its documents below the root, worded; empty when
    /// the folders hold no order.
    pub order: String,
}

/// The last run that can still be undone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunFacts {
    pub when: String,
    pub files: usize,
    pub from: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentsFacts {
    pub documents: u64,
    /// Documents the index has read the text of.
    pub understood: u64,
    pub paused: bool,
    /// Files the index could not read.
    pub unreadable: usize,
    /// Locations whose folder cannot be reached right now.
    pub disconnected: Vec<PathBuf>,
    /// `None` while the folders are being looked at.
    pub areas: Option<Vec<AreaFacts>>,
    pub last_run: Option<RunFacts>,
    pub rules: usize,
    pub ai: AiState,
}

/// What the app reads in the background for the Index and Hermes pages.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoadedFacts {
    pub areas: Vec<AreaFacts>,
    pub last_run: Option<RunFacts>,
    pub disconnected: Vec<PathBuf>,
}
