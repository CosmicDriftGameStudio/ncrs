use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ncrs_core::{CompareMarks, FileEntry, PanelSide};
use ncrs_ui_kit::list_window::ListWindow;

use crate::core::{sync_plan, Choices, Compared, Direction, SyncPlan};

/// The two folders of a comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folders {
    pub left: PathBuf,
    pub right: PathBuf,
}

impl Folders {
    pub fn of(&self, side: PanelSide) -> &Path {
        match side {
            PanelSide::Left => &self.left,
            PanelSide::Right => &self.right,
        }
    }
}

/// A comparison that is running or done.
#[derive(Debug)]
pub struct Run {
    pub folders: Folders,
    /// Results of an older run are dropped.
    pub generation: u64,
    pub stop: Arc<AtomicBool>,
    /// `None` while it is reading.
    pub result: Option<Compared>,
    pub only_differences: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct SyncDialog {
    pub direction: Direction,
    pub delete: bool,
    pub copy_window: ListWindow,
    pub delete_window: ListWindow,
}

/// What follows once the copy queue has done the copies of a sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AfterSync {
    Copying {
        deletions: Vec<PathBuf>,
        side: PanelSide,
    },
    Deleting,
}

#[derive(Debug, Default)]
pub struct Compare {
    /// The choices of the start dialog, kept for the session.
    pub choices: Choices,
    pub start: Option<Folders>,
    pub run: Option<Run>,
    pub sync: Option<SyncDialog>,
    pub after_sync: Option<AfterSync>,
    pub generation: u64,
}

impl Compare {
    /// A dialog of the comparison is on top.
    pub const fn dialog_is_open(&self) -> bool {
        self.start.is_some() || self.sync.is_some()
    }

    /// The marks of the panel on `side`, once the comparison is done.
    pub fn marks(&self, side: PanelSide) -> Option<&CompareMarks> {
        self.compared().map(|compared| compared.marks(side))
    }

    pub fn compared(&self) -> Option<&Compared> {
        self.run.as_ref()?.result.as_ref()
    }

    /// Whether the panels leave out the rows that are the same.
    pub fn filters_rows(&self) -> bool {
        self.run
            .as_ref()
            .is_some_and(|run| run.only_differences && run.result.is_some())
    }

    /// Whether the panel on `side` shows `entry` while the filter is on.
    pub fn shows_row(&self, side: PanelSide, entry: &FileEntry) -> bool {
        match self.compared() {
            Some(compared) if self.filters_rows() => {
                entry.is_parent || compared.row_differs(side, &entry.name)
            }
            _ => true,
        }
    }

    /// The sync the open dialog stands for.
    pub fn plan(&self) -> Option<(Direction, SyncPlan)> {
        let dialog = self.sync.as_ref()?;
        let direction = dialog.direction;
        let compared = self.compared()?;
        let target = self.run.as_ref()?.folders.of(direction.target());
        let delete = dialog.delete && crate::core::can_delete_in(target);
        Some((direction, sync_plan(compared, direction, delete)))
    }

    pub const fn waits_for_ops(&self) -> bool {
        self.after_sync.is_some()
    }

    /// Stops a running comparison and forgets everything of it.
    pub fn end(&mut self) {
        if let Some(run) = self.run.take() {
            run.stop.store(true, Ordering::Relaxed);
        }
        self.sync = None;
        self.after_sync = None;
    }
}
