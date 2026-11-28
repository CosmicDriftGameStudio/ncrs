use std::time::Instant;

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
    /// The moment this message is handled; the page holds its refresh while the
    /// user is moving the cursor.
    pub now: Instant,
}

/// What `view` reads from the app.
#[derive(Debug, Clone, Copy)]
pub struct ViewCtx<'a> {
    /// The used share of the drive, once it is past the warn level.
    pub disk_warning: Option<u8>,
    pub lang: Language,
    pub palette: &'a Palette,
    pub window_size: Size,
}
