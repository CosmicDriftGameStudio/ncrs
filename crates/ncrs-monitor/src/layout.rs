//! The heights on the System page that the row count and the menu position
//! are worked out from. The view gives these regions exactly these heights.

use ncrs_ui_kit::layout::ROW_HEIGHT;
use ncrs_ui_kit::theme::{size, spacing};

/// Host line and meters, with their padding.
pub const TOP_BAND_HEIGHT: f32 = 124.0;
/// The line with the heading, the counts and the filter above the table.
pub const TASKS_HEAD_HEIGHT: f32 = 28.0;
pub const TASKS_HEAD_GAP: f32 = spacing::S;
pub const PAGE_PADDING: f32 = spacing::XL;

/// From the top of the window to the first task row: the title bar of the
/// window, the top band and its rule, the head of the tasks and the table's
/// column header.
pub const ROWS_TOP: f32 = size::TITLE_BAR
    + TOP_BAND_HEIGHT
    + spacing::BORDER_WIDTH
    + PAGE_PADDING
    + TASKS_HEAD_HEIGHT
    + TASKS_HEAD_GAP
    + ROW_HEIGHT;

/// Under the last row: the padding and the status bar.
pub const ROWS_BOTTOM: f32 = PAGE_PADDING + size::STATUS_BAR;

/// How many whole task rows fit; a row that would be cut off is left out.
pub fn visible_rows(window_height: f32) -> usize {
    (((window_height - ROWS_TOP - ROWS_BOTTOM) / ROW_HEIGHT)
        .floor()
        .max(1.0)) as usize
}
