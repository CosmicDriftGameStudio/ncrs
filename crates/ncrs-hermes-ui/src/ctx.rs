use std::path::{Path, PathBuf};
use std::time::SystemTime;

use iced::{Point, Size};
use ncrs_config::file_list_tags::FileListTags;
use ncrs_core::PanelSide;
use ncrs_i18n::Language;
use ncrs_index::Kinds;

/// What `update` reads from the app.
#[derive(Debug, Clone)]
pub struct Ctx<'a> {
    pub lang: Language,
    pub kinds: &'a Kinds,
    pub file_list_tags: &'a FileListTags,
    /// The status page is up. The pages and the menu in [`crate::Hermes`] are
    /// read from the state, which changes while `update` runs.
    pub status_page_open: bool,
    /// A dialog or card is up that nothing opens over, apart from the Ablage
    /// tab and the menu in [`crate::Hermes`].
    pub overlay_apart_from_ablage_tab_open: bool,
    /// The question whether to remove the index and begin again is up.
    pub start_over_dialog_open: bool,
    /// The module is being built again.
    pub hermes_is_restarting: bool,
    /// The counts say that something waits for review.
    pub hermes_has_open_reviews: bool,
    pub context_menu_open: bool,
    pub search_open: bool,
    pub hermes_active: bool,
    pub hermes_counts_missing: bool,
    pub jobs_busy: bool,
    pub window_size: Size,
    pub config_path: Option<&'a Path>,
    /// What a leading `~` in a path stands for.
    pub home: &'a Path,
    /// Blocking; the app swaps it in its tests.
    pub resolve_folder: fn(&Path) -> Result<PathBuf, String>,
    /// The element of a dialog or card that has the focus.
    pub dialog_focus: usize,
    /// The key that opens the status page, worded for a notice.
    pub filing_status_shortcut: Option<String>,
    /// Whether the files of a model are all in this folder.
    pub model_is_installed: fn(&Path) -> bool,
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
