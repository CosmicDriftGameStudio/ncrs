//! Layout metrics the kit components and the app share.
use iced::{Point, Size};
use ncrs_core::Column;

use super::theme::{size, spacing};

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
/// The symbol column: the 16 px symbol and the gap to the next column.
pub const ICON_COLUMN_WIDTH: f32 = 24.0;
pub const FILE_ICON_SIZE: f32 = 16.0;
/// A column dragged narrower or wider stops here.
pub const MIN_COLUMN_WIDTH: f32 = 40.0;
pub const MAX_COLUMN_WIDTH: f32 = 600.0;
/// The strip at the right edge of a title that resizes the column.
pub const COLUMN_GRIP_WIDTH: f32 = 6.0;
/// The line in the accent color where a dragged title would land.
pub const COLUMN_DROP_LINE_WIDTH: f32 = 2.0;

/// The width a column has until the user drags it. The name column has none:
/// it takes what the others leave.
pub fn standard_column_width(column: Column) -> f32 {
    match column {
        Column::Icon => ICON_COLUMN_WIDTH,
        Column::Name => 0.0,
        Column::Size => SIZE_COLUMN_WIDTH,
        Column::Modified | Column::Created | Column::Accessed => DATE_COLUMN_WIDTH,
        Column::Type => 110.0,
        Column::Extension => 80.0,
        Column::Permissions | Column::Owner | Column::Group => 100.0,
        Column::LinkTarget => 200.0,
    }
}
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
/// What the field for the text in files, with its toggles, adds to the header.
pub const SEARCH_CONTENT_EXTRA_HEIGHT: f32 = 32.0;

/// The height of the search header: with filters, their row, hint and the
/// taller summary row come on top; searching in files adds its field.
pub fn search_header_height(filtered: bool, in_files: bool) -> f32 {
    let filters = if filtered {
        SEARCH_FILTER_EXTRA_HEIGHT
    } else {
        0.0
    };
    let content = if in_files {
        SEARCH_CONTENT_EXTRA_HEIGHT
    } else {
        0.0
    };
    SEARCH_HEADER_HEIGHT + filters + content
}

/// Rows of a hit list that fit where `normal_rows` folder rows fit.
pub fn hit_list_rows(normal_rows: usize) -> usize {
    normal_rows.div_euclid(2).max(1)
}

/// Number of result rows of `row_height` that fit into the search view: the
/// panel's room, less the difference between the search header and the column
/// header.
pub fn search_visible_rows(window: Size, filtered: bool, in_files: bool, row_height: f32) -> usize {
    let header = search_header_height(filtered, in_files);
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

/// Where the context menu opens for the cursor row of a panel when it is
/// opened from the keyboard: just below the row, a little into the name column.
///
/// `row_in_view` is the row's position counted from the first visible row.
pub fn cursor_row_anchor(
    right_panel: bool,
    row_in_view: usize,
    row_height: f32,
    window: Size,
) -> Point {
    let panel_width = (window.width - spacing::BORDER_WIDTH) / 2.0;
    let panel_left = if right_panel {
        panel_width + spacing::BORDER_WIDTH
    } else {
        0.0
    };
    let rows_top = size::TITLE_BAR + COLUMN_HEADER_HEIGHT;
    Point::new(
        panel_left + 3.0 * TAG_COLUMN_WIDTH,
        rows_top + (row_in_view + 1) as f32 * row_height,
    )
}
