//! What the Index and Hermes pages show that the app knows and the config file
//! does not. The app fills it when something changes, not on every frame.

use std::collections::BTreeMap;
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

/// The day of a run, from the local date of today.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunDay {
    Today,
    Yesterday,
    /// Older: the date, worded.
    On(String),
}

/// A run of Hermes as the History section lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunFacts {
    /// The batch of the journal, for the undo.
    pub batch: i64,
    pub day: RunDay,
    /// Local time of day, `14:02`.
    pub time: String,
    pub files: usize,
    pub from: String,
    /// The folder the files went to; empty when the journal did not record it.
    pub to: String,
    /// Folders below the target that received files.
    pub examples: Vec<String>,
    /// The run may be put back; a run the journal cannot undo has no button.
    pub undoable: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentsFacts {
    pub documents: u64,
    /// Documents the AI has read and sorted.
    pub understood: u64,
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
    /// The system brings a recognizer for scans and photos (macOS Vision).
    pub system_reads_scans: bool,
}

/// What the app reads in the background for the Index and Hermes pages.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoadedFacts {
    pub areas: Vec<AreaFacts>,
    pub runs: Vec<RunFacts>,
    pub folder_files: BTreeMap<PathBuf, usize>,
    /// Documents the AI read and sorted; `None` without an index.
    pub understood: Option<u64>,
    pub disconnected: Vec<PathBuf>,
}
