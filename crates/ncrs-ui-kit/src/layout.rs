//! Layout metrics the kit components and the app share.
use iced::Size;

use super::theme::size;

pub const INITIAL_WINDOW_SIZE: Size = Size::new(1200.0, 760.0);

pub const FKEYBAR_HEIGHT: f32 = size::FKEY_BAR;

pub const COLUMN_HEADER_HEIGHT: f32 = size::COLUMN_HEADER;

pub use ncrs_core::FUNCTION_KEY_COUNT;

/// A row of a file list. Fixed, so the number of visible rows follows from the
/// window height.
pub const ROW_HEIGHT: f32 = 22.0;
/// A hit list row has the name and, below it, the folder.
pub const HIT_LIST_ROW_HEIGHT: f32 = 2.0 * ROW_HEIGHT;
/// A search result of a content search or the index has a third line.
pub const SEARCH_CONTENT_ROW_HEIGHT: f32 = 3.0 * ROW_HEIGHT;

/// The star column, left of the name. Its own button so a click there tags the
/// row instead of moving the cursor.
pub const TAG_COLUMN_WIDTH: f32 = 22.0;

/// The feature list on the left of the settings tab.
pub const SETTINGS_NAV_WIDTH: f32 = 232.0;
/// The page of the settings tab does not grow wider than this.
pub const SETTINGS_PAGE_MAX_WIDTH: f32 = 760.0;
/// Control column of a setting whose control is a row of options.
pub const SETTINGS_WIDE_CONTROL_WIDTH: f32 = 460.0;

pub const SIZE_COLUMN_WIDTH: f32 = 90.0;
pub const DATE_COLUMN_WIDTH: f32 = 150.0;
/// The Hermes status column, after the date: an icon for a file, a summary
/// of icons and counts for a folder.
pub const HERMES_COLUMN_WIDTH: f32 = 124.0;

/// Space between the date and the Hermes column.
pub const HERMES_CELL_GAP: f32 = 12.0;

/// Vertical space the panel chrome occupies: everything above and below the
/// file rows.
///
/// Single source of truth. The view functions use the same constants when they
/// lay out, and the visible rows use this sum, so a change to any of them moves
/// both sides together. Adding a header without adding it here would silently
/// shorten the list.
pub const CHROME: f32 =
    size::STATUS_BAR + FKEYBAR_HEIGHT + size::COMMAND_LINE + size::TITLE_BAR + size::COLUMN_HEADER;

/// The three fields and the summary line of the search view, above the result
/// rows. Fixed, like the column header it replaces.
pub const SEARCH_HEADER_HEIGHT: f32 = 124.0 + SEARCH_HIT_ROW_HEIGHT + 4.0;
/// The row that opens the hits as a list, below the summary line.
pub const SEARCH_HIT_ROW_HEIGHT: f32 = 28.0;
/// What the filter row, its hint and the taller summary row add to the header.
pub const SEARCH_FILTER_EXTRA_HEIGHT: f32 = 64.0;

/// The height of the search header: with filters, their row, hint and the
/// taller summary row come on top.
pub fn search_header_height(filtered: bool) -> f32 {
    if filtered {
        SEARCH_HEADER_HEIGHT + SEARCH_FILTER_EXTRA_HEIGHT
    } else {
        SEARCH_HEADER_HEIGHT
    }
}

/// Rows of a hit list that fit where `normal_rows` folder rows fit.
pub fn hit_list_rows(normal_rows: usize) -> usize {
    normal_rows.div_euclid(2).max(1)
}

/// Number of result rows of `row_height` that fit into the search view: the
/// panel's room, less the difference between the search header and the column
/// header.
pub fn search_visible_rows(window: Size, filtered: bool, row_height: f32) -> usize {
    let header = search_header_height(filtered);
    let rows = ((window.height - CHROME - header) / row_height).floor();
    (rows as isize).max(1) as usize
}

pub const MENU_ITEM_HEIGHT: f32 = 24.0;
pub const MENU_WIDTH: f32 = 300.0;
/// The Hermes submenu has labels with a note and a key.
pub const MENU_SUB_WIDTH: f32 = 340.0;
pub const MENU_SEPARATOR_HEIGHT: f32 = 7.0;
pub const MENU_PADDING: f32 = 4.0;
/// The quick-fix menu: wider than the context menu for a target path, with a
/// two-line header above its entries.
pub const QUICK_FIX_WIDTH: f32 = 480.0;
pub const QUICK_FIX_HEADER_HEIGHT: f32 = 44.0;

/// Where a span of `length` starts so it stays within `0..limit`: at `anchor`,
/// or ending there when it would not fit.
pub fn place(anchor: f32, length: f32, limit: f32) -> f32 {
    let start = if anchor + length > limit {
        anchor - length
    } else {
        anchor
    };
    start.min(limit - length).max(0.0)
}
