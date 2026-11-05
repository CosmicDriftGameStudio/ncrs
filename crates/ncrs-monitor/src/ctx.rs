use iced::{Point, Size};
use ncrs_i18n::Language;
use ncrs_ui_kit::palette::Palette;

/// What `update` reads from the app.
#[derive(Debug, Clone, Copy)]
pub struct Ctx {
    pub window_size: Size,
    /// Where the pointer was last seen, in window coordinates: a menu opened
    /// by a right click stands there.
    pub pointer: Point,
}

/// What `view` reads from the app.
#[derive(Debug, Clone, Copy)]
pub struct ViewCtx<'a> {
    pub lang: Language,
    pub palette: &'a Palette,
    pub window_size: Size,
}
