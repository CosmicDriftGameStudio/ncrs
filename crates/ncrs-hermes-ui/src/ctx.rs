use std::path::PathBuf;

use iced::{Point, Size};
use ncrs_filing::filing::FilingModule;
use ncrs_i18n::Language;
use ncrs_index::Kinds;

/// What `update` reads from the app.
#[derive(Debug)]
pub struct Ctx<'a> {
    pub lang: Language,
    pub kinds: &'a Kinds,
    pub default_pattern: Option<&'a str>,
    pub filing: Option<&'a FilingModule>,
    /// A page of the Ablage or a dialog is up that nothing opens over, apart
    /// from the pages and the menu in [`crate::Hermes`]: those are read from
    /// the state, which changes while `update` runs.
    pub overlay_apart_from_hermes_open: bool,
    pub context_menu_open: bool,
    pub search_open: bool,
    pub hermes_active: bool,
    pub hermes_counts_missing: bool,
    /// Documents confirmed to be filed again are waiting for a free queue.
    pub refile_pending: bool,
    pub window_size: Size,
    /// Where the quick-fix menu opens: under the cursor row of the active panel.
    pub quick_fix_anchor: Point,
    /// The inquiries under the cursor or among the marked files; read for the
    /// opening of the quick-fix menu only.
    pub inquiry_paths: Vec<PathBuf>,
}
