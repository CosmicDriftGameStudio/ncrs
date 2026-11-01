//! What the app tells the file operations when it calls `update`.

use std::path::Path;
use std::sync::Arc;

use ncrs_core::PanelSide;
use ncrs_fs::{SandboxAccess, Trash};
use ncrs_i18n::Language;

/// The facts about the app that the trash view's update reads.
#[derive(Debug, Clone, Copy)]
pub struct Ctx {
    /// Rows of the trash view that fit the window.
    pub trash_rows: usize,
    /// The latest read of the trash; a listing of another one is dropped.
    pub trash_generation: u64,
}

/// The facts about the app that the flows read: the checks before an operation
/// starts, the words of its notices and the keys they name.
pub struct FlowCtx<'a> {
    pub lang: Language,
    pub trash: &'a Arc<dyn Trash>,
    /// A job runs, or a prompt or a conflict question holds the keyboard.
    pub start_blocked: bool,
    /// The copy or move dialog is up.
    pub transfer_dialog_open: bool,
    /// The status bar shows an error.
    pub has_job_error: bool,
    pub sandbox: &'a SandboxAccess,
    /// The panel that holds the entries an operation acts on.
    pub source_side: PanelSide,
    pub left_archive: Option<&'a Path>,
    pub right_archive: Option<&'a Path>,
    /// The key that deletes for good, spelled for this platform.
    pub permanent_delete_key: Option<String>,
    /// The key that puts the latest deletion back.
    pub restore_last_key: Option<String>,
}

impl std::fmt::Debug for FlowCtx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlowCtx")
            .field("start_blocked", &self.start_blocked)
            .field("source_side", &self.source_side)
            .finish_non_exhaustive()
    }
}

impl FlowCtx<'_> {
    pub fn archive_of(&self, side: PanelSide) -> Option<&Path> {
        match side {
            PanelSide::Left => self.left_archive,
            PanelSide::Right => self.right_archive,
        }
    }
}
