//! Fixed layout metrics. Rows have a fixed height so the number of visible
//! rows can be derived from the window height (used for scrolling).
use iced::{Point, Size};

use super::theme::{size, spacing};

#[cfg(test)]
pub use ncrs_ui_kit::layout::{
    search_visible_rows, FKEYBAR_HEIGHT, HIT_LIST_ROW_HEIGHT, SEARCH_CONTENT_ROW_HEIGHT,
};
pub use ncrs_ui_kit::layout::{
    CHROME, COLUMN_HEADER_HEIGHT, INITIAL_WINDOW_SIZE, ROW_HEIGHT, TAG_COLUMN_WIDTH,
};

pub const STATUSBAR_HEIGHT: f32 = size::STATUS_BAR;
/// The row across the window top that holds the path tabs, the Hermes chip and
/// the Lab button (and the window buttons where the system draws none).
pub const TITLE_BAR_HEIGHT: f32 = size::TITLE_BAR;
/// Where the row starts on macOS: the traffic lights are drawn over its left end.
pub const TRAFFIC_LIGHTS_INSET: f32 = 78.0;

pub const MENU_WIDTH: f32 = 300.0;
/// The Hermes submenu has labels with a note and a key.
pub const MENU_SUB_WIDTH: f32 = 340.0;
pub const MENU_ITEM_HEIGHT: f32 = 24.0;
pub const MENU_SEPARATOR_HEIGHT: f32 = 7.0;
pub const MENU_PADDING: f32 = 4.0;
/// The quick-fix menu: wider than the context menu for a target path, with a
/// two-line header above its entries.
pub const QUICK_FIX_WIDTH: f32 = 480.0;
pub const QUICK_FIX_HEADER_HEIGHT: f32 = 44.0;

/// Number of file rows that fit into a panel for a given window size.
pub fn visible_rows(window: Size) -> usize {
    let rows = ((window.height - CHROME) / ROW_HEIGHT).floor();
    // `max(1)`: a window too small for the chrome still has to show one row,
    // otherwise the panel renders nothing and the user cannot move the cursor.
    (rows as isize).max(1) as usize
}

/// Rows the trash view shows: the panel's, less its title line and the two
/// hint lines at the bottom.
pub fn trash_visible_rows(window: Size) -> usize {
    visible_rows(window).saturating_sub(3).max(1)
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
    let rows_top = TITLE_BAR_HEIGHT + COLUMN_HEADER_HEIGHT;
    Point::new(
        panel_left + 3.0 * TAG_COLUMN_WIDTH,
        rows_top + (row_in_view + 1) as f32 * row_height,
    )
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: `visible_rows` is a hand-computed sum of the chrome heights
    /// around the file rows. Every constant it adds is also used by the view
    /// that draws that element, and nothing tied the two together: changing a
    /// gap in the theme or the layout module left this function quietly wrong.
    ///
    /// These tests pin the arithmetic, so a later change to either side fails
    /// here instead of producing a scroll offset that is off by a row.
    #[test]
    fn a_full_height_window_fits_the_expected_rows() {
        let window = Size::new(1200.0, 760.0);
        // Chrome: status 22 + function keys 24 + command line 24 + title bar 34
        // + column header 22 = 126. 760 - 126 = 634; 634 / 22 = 28.8, so 28 rows.
        assert_eq!(visible_rows(window), 28);
    }

    #[test]
    fn the_search_view_gives_its_header_and_its_row_height_the_rows_they_need() {
        let window = Size::new(1200.0, 760.0);
        // (634 - 156) / 44 = 10.9 and (634 - 156) / 66 = 7.2
        assert_eq!(search_visible_rows(window, false, HIT_LIST_ROW_HEIGHT), 10);
        assert_eq!(
            search_visible_rows(window, false, SEARCH_CONTENT_ROW_HEIGHT),
            7
        );
        // (634 - 220) / 44 = 9.4 and (634 - 220) / 66 = 6.3
        assert_eq!(search_visible_rows(window, true, HIT_LIST_ROW_HEIGHT), 9);
        assert_eq!(
            search_visible_rows(window, true, SEARCH_CONTENT_ROW_HEIGHT),
            6
        );
        assert_eq!(
            search_visible_rows(Size::new(1200.0, 10.0), false, HIT_LIST_ROW_HEIGHT),
            1
        );
    }

    /// The window height the app is opened with, so the number in the startup
    /// layout is the number the first frame draws.
    #[test]
    fn the_initial_window_shows_the_same_rows() {
        assert_eq!(
            visible_rows(INITIAL_WINDOW_SIZE),
            28,
            "the initial size no longer matches the documented row count"
        );
    }

    #[test]
    fn one_row_fits_exactly_at_the_chrome_height() {
        let window = Size::new(800.0, CHROME + ROW_HEIGHT);
        assert_eq!(visible_rows(window), 1);

        // A partial row does not count.
        let a_bit_taller = Size::new(800.0, CHROME + ROW_HEIGHT + 1.0);
        assert_eq!(visible_rows(a_bit_taller), 1);
    }

    /// The chrome sum has to match what the view actually spends, or the last
    /// row of the list is cut off (or a gap is left unused). The constants are
    /// the same ones the view uses, so this catches a header that was added,
    /// resized or forgotten here.
    #[test]
    fn chrome_matches_the_constants_the_view_uses() {
        let expected = STATUSBAR_HEIGHT
            + FKEYBAR_HEIGHT
            + size::COMMAND_LINE
            + TITLE_BAR_HEIGHT
            + COLUMN_HEADER_HEIGHT;
        assert_eq!(
            CHROME, expected,
            "CHROME and the view's constants have drifted apart"
        );
    }

    #[test]
    fn the_keyboard_anchor_is_below_the_row_in_the_right_panel() {
        let window = Size::new(1200.0, 760.0);
        let left = cursor_row_anchor(false, 0, ROW_HEIGHT, window);
        let right = cursor_row_anchor(true, 0, ROW_HEIGHT, window);
        let first_row_top = TITLE_BAR_HEIGHT + COLUMN_HEADER_HEIGHT;
        assert_eq!(left.y, first_row_top + ROW_HEIGHT);
        assert!(right.x > window.width / 2.0);
        assert_eq!(
            cursor_row_anchor(false, 2, ROW_HEIGHT, window).y,
            left.y + 2.0 * ROW_HEIGHT
        );
    }

    /// A window too small for the chrome must still leave room for one row:
    /// the panel renders `entries[scroll_offset..scroll_offset + visible_rows]`
    /// and a zero would make an empty list impossible to leave.
    #[test]
    fn a_tiny_window_still_shows_one_row() {
        for height in [0.0, 10.0, 100.0, 124.0] {
            assert_eq!(
                visible_rows(Size::new(400.0, height)),
                1,
                "a {height}px window should still show one row"
            );
        }
    }

    /// Growing the window must never show fewer rows, and shrinking must never
    /// show more. `ensure_visible` relies on this to clamp `scroll_offset`.
    #[test]
    fn row_count_grows_with_the_window() {
        let mut previous = 0;
        for height in (100..=1200).step_by(20) {
            let rows = visible_rows(Size::new(1200.0, height as f32));
            assert!(
                rows >= previous,
                "row count went down at {height}px: {previous} -> {rows}"
            );
            previous = rows;
        }
    }
}
