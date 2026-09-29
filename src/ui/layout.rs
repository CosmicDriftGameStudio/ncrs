//! Fixed layout metrics. Rows have a fixed height so the number of visible
//! rows can be derived from the window height (used for scrolling).

use iced::Size;

use super::theme::spacing;

pub const INITIAL_WINDOW_SIZE: Size = Size::new(1200.0, 760.0);

pub const HEADER_HEIGHT: f32 = 30.0;
pub const STATUSBAR_HEIGHT: f32 = 28.0;
pub const PANEL_TITLE_HEIGHT: f32 = 28.0;
pub const COLUMN_HEADER_HEIGHT: f32 = 24.0;
pub const ROW_HEIGHT: f32 = 22.0;

pub const SIZE_COLUMN_WIDTH: f32 = 90.0;
pub const DATE_COLUMN_WIDTH: f32 = 150.0;

/// Number of file rows that fit into a panel for a given window size.
pub fn visible_rows(window: Size) -> usize {
    let chrome = HEADER_HEIGHT
        + STATUSBAR_HEIGHT
        + PANEL_TITLE_HEIGHT
        + COLUMN_HEADER_HEIGHT
        + 2.0 * spacing::OUTER_PADDING
        + 2.0 * spacing::SECTION_GAP
        + 2.0 * spacing::BORDER_WIDTH;
    (((window.height - chrome) / ROW_HEIGHT).floor() as isize).max(1) as usize
}
