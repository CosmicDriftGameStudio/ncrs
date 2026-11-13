//! What the viewer takes from the app: data it reads, never state it changes.

use std::path::Path;

use iced::Size;
use ncrs_i18n::Language;
use ncrs_ui_kit::palette::Palette;

/// What `update` reads from the app.
#[derive(Debug, Clone, Copy)]
pub struct Ctx {
    pub lang: Language,
    pub window_size: Size,
}

/// What `view` reads from the app.
#[derive(Debug, Clone, Copy)]
pub struct ViewCtx<'a> {
    pub lang: Language,
    pub palette: &'a Palette,
    pub home: &'a Path,
}
