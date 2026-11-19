//! What the Index and Hermes pages read in the background: the areas of the
//! learned order, the runs of the journal and the figures of the read folders.

use std::collections::BTreeMap;
use std::path::PathBuf;

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

/// The documents below the read folders and how far the AI got with them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReadFolderCoverage {
    pub documents: u64,
    /// Documents that have a text.
    pub with_text: u64,
    /// Documents with a text that the AI has read and sorted.
    pub understood: u64,
}

/// What the app reads in the background for the Index and Hermes pages.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoadedFacts {
    pub areas: Vec<AreaFacts>,
    pub runs: Vec<RunFacts>,
    pub folder_files: BTreeMap<PathBuf, usize>,
    /// What the read folders hold; `None` without an index.
    pub read_folders: Option<ReadFolderCoverage>,
    pub disconnected: Vec<PathBuf>,
    /// The first failure of an index query, when one failed.
    pub error: Option<String>,
}
