//! What the filing tab takes from the app: data it reads, never state it changes.

use std::path::Path;

use iced::Size;
use ncrs_filing::sorting::SortingAccess;
use ncrs_i18n::Language;
use ncrs_ui_kit::palette::Palette;

/// What `update` reads from the app.
#[derive(Debug, Clone)]
pub struct Ctx {
    pub lang: Language,
    /// `None` while the index is not open; then nothing is planned.
    pub access: Option<SortingAccess>,
}

/// What `view` reads from the app.
#[derive(Debug, Clone, Copy)]
pub struct ViewCtx<'a> {
    pub lang: Language,
    pub palette: &'a Palette,
    pub window: Size,
    /// Paths below it are shown with `~`.
    pub home: &'a Path,
}
