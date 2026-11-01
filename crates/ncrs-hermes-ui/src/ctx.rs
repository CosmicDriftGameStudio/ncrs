use std::path::{Path, PathBuf};

use iced::{Point, Size};
use ncrs_config::file_list_tags::FileListTags;
use ncrs_filing::filing::FilingModule;
use ncrs_i18n::Language;
use ncrs_index::Kinds;

use crate::message::HermesPage;

/// What `update` reads from the app.
#[derive(Debug, Clone)]
pub struct Ctx<'a> {
    pub lang: Language,
    pub kinds: &'a Kinds,
    pub default_pattern: Option<&'a str>,
    pub filing: Option<&'a FilingModule>,
    pub file_list_tags: &'a FileListTags,
    /// The overview or the status page is up. The pages and the menu in
    /// [`crate::Hermes`] are read from the state, which changes while `update`
    /// runs.
    pub ablage_tab_apart_from_hermes_open: bool,
    /// A dialog or card is up that nothing opens over, apart from the Ablage
    /// tab and the menu in [`crate::Hermes`].
    pub overlay_apart_from_ablage_tab_open: bool,
    /// The overview or the status page, whichever is up.
    pub page_apart_from_hermes: Option<HermesPage>,
    pub context_menu_open: bool,
    pub search_open: bool,
    pub hermes_active: bool,
    pub hermes_counts_missing: bool,
    /// Documents confirmed to be filed again are waiting for a free queue.
    pub refile_pending: bool,
    pub window_size: Size,
    pub active_panel_path: &'a Path,
    pub other_panel_path: &'a Path,
    /// Where the quick-fix menu opens: under the cursor row of the active panel.
    pub quick_fix_anchor: Point,
    /// The inquiries under the cursor or among the marked files; read for the
    /// opening of the quick-fix menu only.
    pub inquiry_paths: Vec<PathBuf>,
}
