use std::path::{Path, PathBuf};
use std::time::SystemTime;

use iced::{Point, Size};
use ncrs_config::file_list_tags::FileListTags;
use ncrs_core::PanelSide;
use ncrs_filing::chat_work::ChatAccess;
use ncrs_i18n::Language;
use ncrs_index::Kinds;
use std::sync::Arc;

/// What filing by itself needs: the handle of the chat and the model, which
/// are there only while Hermes is ready and its AI is on.
#[derive(Clone)]
pub struct ContinuousTools {
    pub access: ChatAccess,
    pub model: Arc<dyn ncrs_ai::ChatModel>,
}

impl std::fmt::Debug for ContinuousTools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContinuousTools").finish_non_exhaustive()
    }
}

/// What `update` reads from the app.
#[derive(Debug, Clone)]
pub struct Ctx<'a> {
    pub lang: Language,
    pub kinds: &'a Kinds,
    pub file_list_tags: &'a FileListTags,
    /// The question whether to remove the index and begin again is up.
    pub start_over_dialog_open: bool,
    /// The module is being built again.
    pub hermes_is_restarting: bool,
    /// The counts say that something waits for review.
    pub hermes_has_open_reviews: bool,
    pub context_menu_open: bool,
    pub search_open: bool,
    pub hermes_active: bool,
    /// The Hermes chat is open.
    pub chat_open: bool,
    pub hermes_counts_missing: bool,
    pub jobs_busy: bool,
    pub continuous: Option<ContinuousTools>,
    /// The source folder of the "Ablegen" tab while it plans or files.
    pub filing_tab_source: Option<&'a Path>,
    pub window_size: Size,
    pub config_path: Option<&'a Path>,
    /// What a leading `~` in a path stands for.
    pub home: &'a Path,
    /// The key that opens the status page, worded for a notice.
    pub filing_status_shortcut: Option<String>,
    pub active_panel_path: &'a Path,
    pub other_panel_path: &'a Path,
    /// Where the quick-fix menu opens: under the cursor row of the active panel.
    pub quick_fix_anchor: Point,
    /// The inquiries under the cursor or among the marked files; read for the
    /// opening of the quick-fix menu only.
    pub inquiry_paths: Vec<PathBuf>,
    /// The wall clock the status page measures the time of a worker by.
    pub now: SystemTime,
    /// The left and the right panel, in this order.
    pub panels: [PanelFacts<'a>; 2],
    /// The entry under the cursor of the active panel; `None` for a virtual
    /// panel or an empty one.
    pub cursor_entry: Option<&'a Path>,
    /// The row under the cursor of the active panel, not the `..` row, and
    /// whether it is a folder: what the Hermes file commands act on.
    pub cursor_row: Option<(&'a Path, bool)>,
    /// A dialog or menu lies over the panels, so the panel keys are not theirs.
    pub overlay_open: bool,
    /// The files the F5/F6/F8 commands would act on; filled only for "process
    /// again".
    pub action_sources: Vec<PathBuf>,
}

impl<'a> Ctx<'a> {
    pub fn panel(&self, side: PanelSide) -> Option<&PanelFacts<'a>> {
        self.panels.iter().find(|panel| panel.side == side)
    }
}

/// What the column of Hermes reads of a panel.
#[derive(Debug, Clone)]
pub struct PanelFacts<'a> {
    pub side: PanelSide,
    pub directory: &'a Path,
    pub request_id: u64,
    /// The panel is not loading and shows a real directory.
    pub readable: bool,
    /// The rows in view.
    pub visible: Vec<VisibleEntry<'a>>,
}

#[derive(Debug, Clone, Copy)]
pub struct VisibleEntry<'a> {
    pub path: &'a Path,
    pub is_dir: bool,
    pub is_parent: bool,
}
