//! The window a long list shows: which row is the first one, moved by the
//! wheel or the trackpad and kept around the cursor. The rows are drawn from
//! the window, not by a `scrollable`, so thousands of rows cost nothing.

use iced::mouse::ScrollDelta;

use crate::layout::ROW_HEIGHT;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ListWindow {
    offset: usize,
    /// Trackpad pixels that do not yet add up to a whole row, in rows.
    remainder: f32,
}

impl ListWindow {
    /// The first row shown, inside the list.
    pub fn offset(&self, len: usize, visible: usize) -> usize {
        self.offset.min(len.saturating_sub(visible.max(1)))
    }

    /// Scrolls by a wheel or trackpad movement, in the direction iced reports
    /// it (positive `y` is towards the top). Trackpad pixels are collected
    /// until they make a whole row; the lines of a wheel count as rows. The
    /// cursor stays where it is while it is in view and is held at the edge
    /// otherwise.
    pub fn scroll(&mut self, delta: ScrollDelta, len: usize, visible: usize, cursor: &mut usize) {
        let rows_down = match delta {
            ScrollDelta::Lines { y, .. } => -y,
            ScrollDelta::Pixels { y, .. } => -y / ROW_HEIGHT,
        } + self.remainder;
        let whole = rows_down.trunc();
        self.remainder = rows_down - whole;
        let rows = visible.max(1);
        let max_offset = len.saturating_sub(rows);
        self.offset = self
            .offset(len, visible)
            .saturating_add_signed(whole as isize)
            .min(max_offset);
        let at_top = self.offset == 0 && whole < 0.0;
        let at_bottom = self.offset == max_offset && whole > 0.0;
        if at_top || at_bottom {
            self.remainder = 0.0;
        }
        *cursor = (*cursor)
            .clamp(self.offset, self.offset + rows - 1)
            .min(len.saturating_sub(1));
    }

    /// Moves the window so the cursor is inside it.
    pub fn ensure_visible(&mut self, cursor: usize, len: usize, visible: usize) {
        let rows = visible.max(1);
        if cursor < self.offset {
            self.offset = cursor;
        } else if cursor >= self.offset + rows {
            self.offset = cursor + 1 - rows;
        }
        self.offset = self.offset(len, visible);
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    const VISIBLE: usize = 5;

    fn pixels(y: f32) -> ScrollDelta {
        ScrollDelta::Pixels { x: 0.0, y }
    }

    fn lines(y: f32) -> ScrollDelta {
        ScrollDelta::Lines { x: 0.0, y }
    }

    #[test]
    fn small_trackpad_movements_add_up_to_a_row() {
        let mut window = ListWindow::default();
        let mut cursor = 0;
        for _ in 0..3 {
            window.scroll(pixels(-ROW_HEIGHT / 3.0 - 0.01), 100, VISIBLE, &mut cursor);
        }
        assert_eq!(window.offset(100, VISIBLE), 1);
    }

    #[test]
    fn the_wheel_moves_by_lines_towards_the_end_and_back() {
        let mut window = ListWindow::default();
        let mut cursor = 0;
        window.scroll(lines(-3.0), 100, VISIBLE, &mut cursor);
        assert_eq!(window.offset(100, VISIBLE), 3);
        window.scroll(lines(1.0), 100, VISIBLE, &mut cursor);
        assert_eq!(window.offset(100, VISIBLE), 2);
    }

    #[test]
    fn the_cursor_is_held_at_the_edge_of_the_window() {
        let mut window = ListWindow::default();
        let mut cursor = 1;
        window.scroll(lines(-10.0), 100, VISIBLE, &mut cursor);
        assert_eq!((window.offset(100, VISIBLE), cursor), (10, 10));
        window.scroll(lines(20.0), 100, VISIBLE, &mut cursor);
        assert_eq!((window.offset(100, VISIBLE), cursor), (0, 4));
    }

    #[test]
    fn the_window_stops_at_both_ends_of_the_list() {
        let mut window = ListWindow::default();
        let mut cursor = 0;
        window.scroll(lines(-1000.0), 20, VISIBLE, &mut cursor);
        assert_eq!((window.offset(20, VISIBLE), cursor), (15, 15));
        window.scroll(lines(1000.0), 20, VISIBLE, &mut cursor);
        assert_eq!((window.offset(20, VISIBLE), cursor), (0, 4));
    }

    #[test]
    fn a_list_shorter_than_the_window_does_not_move() {
        let mut window = ListWindow::default();
        let mut cursor = 1;
        window.scroll(lines(-5.0), 3, VISIBLE, &mut cursor);
        assert_eq!((window.offset(3, VISIBLE), cursor), (0, 1));
        window.scroll(lines(-5.0), 0, VISIBLE, &mut cursor);
        assert_eq!(cursor, 0);
    }

    #[test]
    fn moving_the_cursor_brings_it_into_view() {
        let mut window = ListWindow::default();
        window.ensure_visible(7, 100, VISIBLE);
        assert_eq!(window.offset(100, VISIBLE), 3);
        window.ensure_visible(4, 100, VISIBLE);
        assert_eq!(window.offset(100, VISIBLE), 3);
        window.ensure_visible(2, 100, VISIBLE);
        assert_eq!(window.offset(100, VISIBLE), 2);
        window.ensure_visible(99, 100, VISIBLE);
        assert_eq!(window.offset(100, VISIBLE), 95);
    }

    #[test]
    fn a_shorter_list_pulls_the_window_back() {
        let mut window = ListWindow::default();
        window.ensure_visible(50, 100, VISIBLE);
        assert_eq!(window.offset(10, VISIBLE), 5);
    }
}
