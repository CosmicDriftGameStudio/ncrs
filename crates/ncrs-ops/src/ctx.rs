//! What the app tells the file operations when it calls `update`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ncrs_core::PanelSide;
use ncrs_fs::remote::session::Pool;
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
    /// A job runs.
    pub jobs_busy: bool,
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
    /// The root address of the server each panel shows a folder of.
    pub left_server: Option<PathBuf>,
    pub right_server: Option<PathBuf>,
    /// The connections to the servers.
    pub remote: &'a Arc<Pool>,
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
    /// Whether a panel is in `place`: an archive, or the root address of the
    /// server whose folder it shows.
    pub fn panel_shows(&self, place: &Path) -> bool {
        self.left_archive == Some(place)
            || self.right_archive == Some(place)
            || self.left_server.as_deref() == Some(place)
            || self.right_server.as_deref() == Some(place)
    }

    pub fn archive_of(&self, side: PanelSide) -> Option<&Path> {
        match side {
            PanelSide::Left => self.left_archive,
            PanelSide::Right => self.right_archive,
        }
    }
}
