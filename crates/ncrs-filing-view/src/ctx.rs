//! What the filing tab takes from the app: data it reads, never state it changes.

use std::sync::Arc;

use iced::Size;
use ncrs_ai::ChatModel;
use ncrs_filing::chat_work::ChatAccess;
use ncrs_i18n::Language;
use ncrs_ui_kit::palette::Palette;

/// What `update` reads from the app.
#[derive(Debug, Clone)]
pub struct Ctx {
    pub lang: Language,
    /// With the model set; without it nothing is planned.
    pub access: Option<ChatAccess>,
    pub model: Option<Arc<dyn ChatModel>>,
}

/// What `view` reads from the app.
#[derive(Debug, Clone, Copy)]
pub struct ViewCtx<'a> {
    pub lang: Language,
    pub palette: &'a Palette,
    pub window: Size,
}
