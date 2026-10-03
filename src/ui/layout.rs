//! Fixed layout metrics. Rows have a fixed height so the number of visible
//! rows can be derived from the window height (used for scrolling).
use iced::{Point, Size};

use super::theme::spacing;

pub const INITIAL_WINDOW_SIZE: Size = Size::new(1200.0, 760.0);

pub const HEADER_HEIGHT: f32 = 30.0;
pub const STATUSBAR_HEIGHT: f32 = 28.0;
pub const FKEYBAR_HEIGHT: f32 = 24.0;
pub const PANEL_TITLE_HEIGHT: f32 = 28.0;
pub const COLUMN_HEADER_HEIGHT: f32 = 24.0;
pub const ROW_HEIGHT: f32 = 22.0;

pub const DIALOG_WIDTH: f32 = 420.0;

pub const MENU_WIDTH: f32 = 300.0;
pub const MENU_ITEM_HEIGHT: f32 = 24.0;
pub const MENU_SEPARATOR_HEIGHT: f32 = 7.0;
pub const MENU_PADDING: f32 = 4.0;

/// The star column, left of the name. Its own button so a click there tags the
/// row instead of moving the cursor.
pub const TAG_COLUMN_WIDTH: f32 = 22.0;

pub const SIZE_COLUMN_WIDTH: f32 = 90.0;
pub const DATE_COLUMN_WIDTH: f32 = 150.0;

/// Gaps between the four stacked bars of the window: header, panels, status
/// bar and function key bar.
const COLUMN_GAPS: f32 = 3.0;

/// Vertical space the panel chrome occupies: everything above and below the
/// file rows.
///
/// Single source of truth. The view functions use the same constants when they
/// lay out, and `visible_rows` uses this sum, so a change to any of them moves
/// both sides together. Adding a header without adding it here would silently
/// shorten the list, which is what the tests below guard against.
const CHROME: f32 = HEADER_HEIGHT
    + STATUSBAR_HEIGHT
    + FKEYBAR_HEIGHT
    + PANEL_TITLE_HEIGHT
    + COLUMN_HEADER_HEIGHT
    + 2.0 * spacing::OUTER_PADDING
    + COLUMN_GAPS * spacing::SECTION_GAP
    + 2.0 * spacing::BORDER_WIDTH;

/// Number of file rows that fit into a panel for a given window size.
pub fn visible_rows(window: Size) -> usize {
    let rows = ((window.height - CHROME) / ROW_HEIGHT).floor();
    // `max(1)`: a window too small for the chrome still has to show one row,
    // otherwise the panel renders nothing and the user cannot move the cursor.
    (rows as isize).max(1) as usize
}

/// Where the context menu opens for the cursor row of a panel when it is
/// opened from the keyboard: just below the row, a little into the name column.
///
/// `row_in_view` is the row's position counted from the first visible row.
pub fn cursor_row_anchor(right_panel: bool, row_in_view: usize, window: Size) -> Point {
    let panel_width = (window.width - 2.0 * spacing::OUTER_PADDING - spacing::PANEL_GAP) / 2.0;
    let panel_left = spacing::OUTER_PADDING
        + if right_panel {
            panel_width + spacing::PANEL_GAP
        } else {
            0.0
        };
    let rows_top = spacing::OUTER_PADDING
        + HEADER_HEIGHT
        + spacing::SECTION_GAP
        + spacing::BORDER_WIDTH
        + PANEL_TITLE_HEIGHT
        + COLUMN_HEADER_HEIGHT;
    Point::new(
        panel_left + 3.0 * TAG_COLUMN_WIDTH,
        rows_top + (row_in_view + 1) as f32 * ROW_HEIGHT,
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
        // Chrome: header 30 + status 28 + function keys 24 + title 28 + column
        // header 24 + padding 12 + three gaps 18 + borders 2 = 166.
        // 760 - 166 = 594; 594 / 22 = 27 rows exactly.
        assert_eq!(visible_rows(window), 27);
    }

    /// The window height the app is opened with, so the number in the startup
    /// layout is the number the first frame draws.
    #[test]
    fn the_initial_window_shows_the_same_rows() {
        assert_eq!(
            visible_rows(INITIAL_WINDOW_SIZE),
            27,
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
        let expected = HEADER_HEIGHT
            + STATUSBAR_HEIGHT
            + FKEYBAR_HEIGHT
            + PANEL_TITLE_HEIGHT
            + COLUMN_HEADER_HEIGHT
            + 2.0 * spacing::OUTER_PADDING
            + 3.0 * spacing::SECTION_GAP
            + 2.0 * spacing::BORDER_WIDTH;
        assert_eq!(
            CHROME, expected,
            "CHROME and the view's constants have drifted apart"
        );
    }

    #[test]
    fn the_keyboard_anchor_is_below_the_row_in_the_right_panel() {
        let window = Size::new(1200.0, 760.0);
        let left = cursor_row_anchor(false, 0, window);
        let right = cursor_row_anchor(true, 0, window);
        // 6 + 30 + 6 + 1 + 28 + 24 = 95 is where the first row starts.
        assert_eq!(left.y, 95.0 + ROW_HEIGHT);
        assert!(right.x > window.width / 2.0);
        assert_eq!(
            cursor_row_anchor(false, 2, window).y,
            left.y + 2.0 * ROW_HEIGHT
        );
    }

    /// A window too small for the chrome must still leave room for one row:
    /// the panel renders `entries[scroll_offset..scroll_offset + visible_rows]`
    /// and a zero would make an empty list impossible to leave.
    #[test]
    fn a_tiny_window_still_shows_one_row() {
        for height in [0.0, 10.0, 100.0, 136.0, 166.0] {
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
