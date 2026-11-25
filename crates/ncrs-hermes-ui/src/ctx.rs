use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ncrs_config::file_list_tags::FileListTags;
use ncrs_core::PanelSide;
use ncrs_i18n::Language;
use ncrs_index::Kinds;
use std::sync::Arc;

/// The trash the app deletes into, so what an answer removes can be got back.
#[derive(Clone)]
pub struct Bin(pub Arc<dyn ncrs_fs::Trash>);

impl std::fmt::Debug for Bin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bin").finish_non_exhaustive()
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
    pub search_open: bool,
    pub jobs_busy: bool,
    /// The watched sources may be filed from: a locked trial holds the
    /// background filing back without a word.
    pub watching_allowed: bool,
    pub trash: Bin,
    /// The source folder of the "Ablegen" tab while it plans or files.
    pub filing_tab_source: Option<&'a Path>,
    pub config_path: Option<&'a Path>,
    /// What a leading `~` in a path stands for.
    pub home: &'a Path,
    /// The key that opens the status page, worded for a notice.
    pub filing_status_shortcut: Option<String>,
    pub active_panel_path: &'a Path,
    pub other_panel_path: &'a Path,
    /// The wall clock the status page measures the time of a worker by.
    pub now: SystemTime,
    /// The left and the right panel, in this order.
    pub panels: [PanelFacts<'a>; 2],
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
