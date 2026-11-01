//! What the panels take from the app: data they read, never state they change.

use std::path::{Path, PathBuf};
use std::time::Instant;

use iced::keyboard::Modifiers;
use iced::{Point, Size};
use ncrs_core::{FileEntry, PanelSide};
use ncrs_i18n::Language;
use ncrs_ui_kit::file_row::HermesCell;
use ncrs_ui_kit::palette::Palette;

use crate::message::PanelMessage;

/// What `update` reads from the app.
pub struct Ctx<'a> {
    /// Rows that fit into a panel.
    pub visible_rows: usize,
    pub window_size: Size,
    pub modifiers: Modifiers,
    /// Whether the held modifiers turn a click into a right click.
    pub is_context_click: bool,
    /// Where the pointer is in the window.
    pub pointer: Point,
    /// A dialog, page or menu is on top of the panels.
    pub overlay_is_open: bool,
    pub context_menu_is_open: bool,
    pub clock: &'a (dyn Fn() -> Instant + Send + Sync),
    /// The first of a path and its ancestors that is a folder.
    pub nearest_existing_directory: fn(&Path) -> Option<PathBuf>,
}

impl std::fmt::Debug for Ctx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ctx")
            .field("visible_rows", &self.visible_rows)
            .field("window_size", &self.window_size)
            .field("modifiers", &self.modifiers)
            .field("is_context_click", &self.is_context_click)
            .field("pointer", &self.pointer)
            .field("overlay_is_open", &self.overlay_is_open)
            .field("context_menu_is_open", &self.context_menu_is_open)
            .finish_non_exhaustive()
    }
}

/// The Hermes cell of a row, built by the app from what the Hermes column knows.
pub type HermesCellOf<'a> = Box<dyn Fn(&FileEntry) -> HermesCell<'a, PanelMessage> + 'a>;

/// What `view` reads from the app.
pub struct ViewCtx<'a> {
    pub side: PanelSide,
    pub lang: Language,
    pub palette: Palette,
    pub is_active: bool,
    pub visible_rows: usize,
    /// `None` leaves the Hermes column out.
    pub hermes_cell: Option<HermesCellOf<'a>>,
}

impl std::fmt::Debug for ViewCtx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewCtx")
            .field("side", &self.side)
            .field("lang", &self.lang)
            .field("is_active", &self.is_active)
            .field("visible_rows", &self.visible_rows)
            .field("has_hermes_column", &self.hermes_cell.is_some())
            .finish_non_exhaustive()
    }
}

/// Ctrl+click is the Mac's right click.
pub fn is_context_click(modifiers: Modifiers) -> bool {
    cfg!(target_os = "macos") && modifiers.control()
}
