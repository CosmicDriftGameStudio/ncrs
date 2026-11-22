//! What the Git tab takes from the app: data it reads, never state it changes.

use iced::Size;
use ncrs_i18n::Language;
use ncrs_ui_kit::format::DateFormat;
use ncrs_ui_kit::palette::Palette;

/// What `update` reads from the app.
#[derive(Debug, Clone, Copy)]
pub struct Ctx {
    pub lang: Language,
}

/// What `view` reads from the app.
#[derive(Debug, Clone, Copy)]
pub struct ViewCtx<'a> {
    pub lang: Language,
    pub palette: &'a Palette,
    pub date_format: DateFormat,
    /// The window: the tab fills it.
    pub window: Size,
}
