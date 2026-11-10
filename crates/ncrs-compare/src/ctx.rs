use std::path::Path;
use std::sync::Arc;

use ncrs_fs::remote::session::Pool;
use ncrs_i18n::Language;

/// What `update` reads from the app.
#[derive(Debug)]
pub struct Ctx<'a> {
    pub lang: Language,
    pub pool: &'a Arc<Pool>,
    /// The folders the panels show now.
    pub left: &'a Path,
    pub right: &'a Path,
    /// A page or a dialog is up that nothing opens over.
    pub blocking_overlay_open: bool,
}
