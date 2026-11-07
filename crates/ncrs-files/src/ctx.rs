//! What the panels take from the app: data they read, never state they change.

use std::path::{Path, PathBuf};
use std::time::Instant;

use iced::keyboard::Modifiers;
use iced::{Point, Size};
use ncrs_core::{Column, DateFormat, FileEntry, PanelSide, SizeFormat};
use ncrs_i18n::Language;
use ncrs_ui_kit::column_header::{ColumnLayout, HeaderDrag};
use ncrs_ui_kit::file_icon::SpecialFolder;
use ncrs_ui_kit::file_row::HermesCell;
use ncrs_ui_kit::palette::Palette;

use crate::message::PanelMessage;

/// The `[columns]` setting as the panels read it.
#[derive(Debug, Clone, Copy)]
pub struct ColumnSettings<'a> {
    pub same_in_both: bool,
    /// In display order, as stored: columns this system cannot show are in
    /// the list and are left out when drawing.
    pub left: &'a [Column],
    pub right: &'a [Column],
    /// Pixels; a column without an entry has its standard width.
    pub widths: &'a [(Column, u16)],
    pub date_format: DateFormat,
    pub size_format: SizeFormat,
}

impl ColumnSettings<'static> {
    pub const STANDARD: Self = Self {
        same_in_both: true,
        left: &Column::DEFAULT,
        right: &Column::DEFAULT,
        widths: &[],
        date_format: DateFormat::Iso,
        size_format: SizeFormat::Readable,
    };
}

impl<'a> ColumnSettings<'a> {
    /// The columns of the panel on `side`; the left list for both when the
    /// panels are the same.
    pub fn of(&self, side: PanelSide) -> &'a [Column] {
        match side {
            PanelSide::Right if !self.same_in_both => self.right,
            PanelSide::Left | PanelSide::Right => self.left,
        }
    }

    pub fn layout(&self, side: PanelSide, lang: Language) -> ColumnLayout<'a> {
        ColumnLayout {
            columns: self.of(side),
            widths: self.widths,
            date_format: self.date_format,
            size_format: self.size_format,
            lang,
        }
    }
}

/// What `update` reads from the app.
pub struct Ctx<'a> {
    /// Which columns the panels show and how wide they are.
    pub columns: ColumnSettings<'a>,
    /// The Hermes column is drawn after the others, which narrows the name.
    pub has_hermes_column: bool,
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
    pub columns: ColumnSettings<'a>,
    /// Folders that get a symbol of their own, by path: home, desktop,
    /// downloads, and the folders Hermes reads and files.
    pub special_folders: &'a [(PathBuf, SpecialFolder)],
    /// The title being dragged in this panel's header, from
    /// [`crate::state::Panels::header_drag_view`].
    pub header_drag: Option<HeaderDrag>,
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
