//! What the flows of the file operations keep between two messages.

use std::path::PathBuf;

use ncrs_core::PanelSide;
use ncrs_fs::transfer::OnConflict;
use ncrs_fs::TrashedItem;

/// What a delete works on, resolved once when F8 is pressed: by the time the
/// user has answered the dialog the cursor may have moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deletion {
    pub sources: Vec<PathBuf>,
    /// The permanent-delete key (Shift+F8 by default): no trash.
    pub permanent: bool,
    /// The panel the entries came from, which is cleared and reloaded at the
    /// end even if the user switches panels meanwhile.
    pub side: PanelSide,
}

/// A put-back from the trash, resolved when Enter is pressed: by the time a
/// name turns out to be taken the cursor may have moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restoration {
    pub items: Vec<TrashedItem>,
    pub restored: usize,
    pub skipped: usize,
    /// An "all" answer to the dialog, applied to the rows after it.
    pub rule: Option<OnConflict>,
}

/// The state of the delete and restore flows and the count that tells a late result from a
/// current one. Every operation of the app bumps `generation` when it starts or
/// ends, so a result that arrives after that is dropped.
#[derive(Debug, Default)]
pub struct Ops {
    /// The delete confirmation dialog, while it is up.
    pub delete_dialog: Option<Deletion>,
    /// The delete that is running.
    pub deleting: Option<Deletion>,
    /// What the latest F8 run moved to the trash, for the restore key.
    pub last_deletion: Vec<PathBuf>,
    /// A restore from the trash, while it runs.
    pub restoring: Option<Restoration>,
    /// The restore paused on a name that is taken: the index of its row.
    pub restore_conflict: Option<usize>,
    pub generation: u64,
}

impl Ops {
    pub fn bump_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }
}
