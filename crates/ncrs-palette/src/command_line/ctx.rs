use std::path::{Path, PathBuf};

use ncrs_core::PanelSide;
use ncrs_i18n::Language;

/// What the command line reads from the app.
#[derive(Debug)]
pub struct CommandLineCtx<'a> {
    /// A dialog or a page covers the panels: the line cannot take the focus.
    pub blocked: bool,
    pub lang: Language,
    /// The active panel shows a server.
    pub remote: bool,
    pub active_side: PanelSide,
    /// The directory the active panel shows; a typed `cd` starts there.
    pub active_path: PathBuf,
    pub home: &'a Path,
    /// Where a shell command of the active panel runs.
    pub shell_directory: PathBuf,
}
