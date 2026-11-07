//! A press on a column title: a click that sorts, a drag that moves the
//! column, or a drag at its right edge that resizes it. Which of them it is
//! shows only from what the pointer does next.

use iced::Point;
use ncrs_core::{Column, PanelSide};
use ncrs_ui_kit::column_header::{ColumnSpan, HeaderDrag};
use ncrs_ui_kit::layout::{MAX_COLUMN_WIDTH, MIN_COLUMN_WIDTH};
use ncrs_ui_kit::theme::spacing;

use crate::panel_drag::DRAG_THRESHOLD;

/// What the press has become so far.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HeaderPhase {
    /// Not moved yet: letting go is a click.
    Pressed,
    Reordering {
        /// How far right of the title's left edge the pointer took hold.
        grab_offset: f32,
        /// Left edge of the title as drawn, counted from the panel's edge.
        ghost_x: f32,
        /// Index among the shown columns the title would land before.
        drop_before: usize,
        /// The column it would land before; `None` at the end.
        before: Option<Column>,
    },
    Resizing {
        start_width: f32,
        width: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeaderPress {
    pub side: PanelSide,
    pub column: Column,
    pub origin: Point,
    pub phase: HeaderPhase,
}

/// Width of one panel; the same split the app makes of the window.
pub fn panel_width(window_width: f32) -> f32 {
    (window_width - spacing::BORDER_WIDTH) / 2.0
}

/// Where the panel on `side` starts, counted from the left edge of the window.
pub fn panel_left(side: PanelSide, window_width: f32) -> f32 {
    match side {
        PanelSide::Left => 0.0,
        PanelSide::Right => panel_width(window_width) + spacing::BORDER_WIDTH,
    }
}

/// The index among `spans` a title dropped with the pointer at `local_x`
/// (counted from the panel's edge) lands before: after the columns whose
/// middle lies left of the pointer.
pub fn drop_index(spans: &[ColumnSpan], local_x: f32) -> usize {
    spans
        .iter()
        .position(|span| local_x < span.left + span.width / 2.0)
        .unwrap_or(spans.len())
}

impl HeaderPress {
    pub fn pressed(side: PanelSide, column: Column, origin: Point) -> Self {
        Self {
            side,
            column,
            origin,
            phase: HeaderPhase::Pressed,
        }
    }

    pub fn resizing(side: PanelSide, column: Column, origin: Point, start_width: f32) -> Self {
        Self {
            side,
            column,
            origin,
            phase: HeaderPhase::Resizing {
                start_width,
                width: start_width,
            },
        }
    }

    /// The pointer moved to `position`. `spans` are the shown columns of the
    /// panel as they lie now.
    pub fn moved_to(&mut self, position: Point, spans: &[ColumnSpan], window_width: f32) {
        let left = panel_left(self.side, window_width);
        match self.phase {
            HeaderPhase::Pressed => {
                if self.origin.distance(position) < DRAG_THRESHOLD {
                    return;
                }
                let grab_offset = spans
                    .iter()
                    .find(|span| span.column == self.column)
                    .map_or(0.0, |span| (self.origin.x - left - span.left).max(0.0));
                self.phase = HeaderPhase::Reordering {
                    grab_offset,
                    ghost_x: 0.0,
                    drop_before: 0,
                    before: None,
                };
                self.moved_to(position, spans, window_width);
            }
            HeaderPhase::Reordering { grab_offset, .. } => {
                let local_x = position.x - left;
                let drop_before = drop_index(spans, local_x);
                self.phase = HeaderPhase::Reordering {
                    grab_offset,
                    ghost_x: local_x - grab_offset,
                    drop_before,
                    before: spans.get(drop_before).map(|span| span.column),
                };
            }
            HeaderPhase::Resizing { start_width, .. } => {
                self.phase = HeaderPhase::Resizing {
                    start_width,
                    width: (start_width + position.x - self.origin.x)
                        .clamp(MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH),
                };
            }
        }
    }

    /// What the header draws for this press; nothing while it is still a click.
    pub fn view(&self) -> Option<HeaderDrag> {
        match self.phase {
            HeaderPhase::Pressed => None,
            HeaderPhase::Reordering {
                ghost_x,
                drop_before,
                ..
            } => Some(HeaderDrag::Reorder {
                column: self.column,
                ghost_x,
                drop_before,
            }),
            HeaderPhase::Resizing { width, .. } => Some(HeaderDrag::Resize {
                column: self.column,
                width,
            }),
        }
    }
}

/// `columns` with `dragged` moved in front of `before`, or to the end.
pub fn reordered(columns: &[Column], dragged: Column, before: Option<Column>) -> Vec<Column> {
    if before == Some(dragged) {
        return columns.to_vec();
    }
    let mut moved: Vec<Column> = columns
        .iter()
        .copied()
        .filter(|column| *column != dragged)
        .collect();
    let at = before
        .and_then(|target| moved.iter().position(|column| *column == target))
        .unwrap_or(moved.len());
    moved.insert(at, dragged);
    moved
}

/// `columns` with `column` shown if it was not and hidden if it was. The name
/// stays; the symbol goes to the front, the others to the end.
pub fn toggled(columns: &[Column], column: Column) -> Vec<Column> {
    if column == Column::Name {
        return columns.to_vec();
    }
    if columns.contains(&column) {
        return columns
            .iter()
            .copied()
            .filter(|shown| *shown != column)
            .collect();
    }
    let mut with = columns.to_vec();
    if column == Column::Icon {
        with.insert(0, column);
    } else {
        with.push(column);
    }
    with
}

// A failing assertion in a test is the signal, so `unwrap` belongs here.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    fn spans() -> Vec<ColumnSpan> {
        let widths = [
            (Column::Name, 300.0),
            (Column::Size, 100.0),
            (Column::Modified, 100.0),
        ];
        let mut left = 0.0;
        widths
            .into_iter()
            .map(|(column, width)| {
                let span = ColumnSpan {
                    column,
                    left,
                    width,
                };
                left += width;
                span
            })
            .collect()
    }

    #[test]
    fn a_title_lands_before_the_column_whose_middle_is_right_of_the_pointer() {
        let spans = spans();
        assert_eq!(drop_index(&spans, 10.0), 0);
        assert_eq!(drop_index(&spans, 160.0), 1);
        assert_eq!(drop_index(&spans, 340.0), 1);
        assert_eq!(drop_index(&spans, 360.0), 2);
        assert_eq!(drop_index(&spans, 900.0), 3);
    }

    #[test]
    fn moving_a_column_puts_it_before_the_target_or_at_the_end() {
        let columns = [Column::Icon, Column::Name, Column::Size, Column::Modified];
        assert_eq!(
            reordered(&columns, Column::Modified, Some(Column::Name)),
            [Column::Icon, Column::Modified, Column::Name, Column::Size]
        );
        assert_eq!(
            reordered(&columns, Column::Icon, None),
            [Column::Name, Column::Size, Column::Modified, Column::Icon]
        );
        assert_eq!(
            reordered(&columns, Column::Size, Some(Column::Size)),
            columns
        );
        assert_eq!(
            reordered(&columns, Column::Size, Some(Column::Modified)),
            columns
        );
    }

    #[test]
    fn a_column_is_shown_and_hidden_but_never_the_name() {
        let columns = [Column::Name, Column::Size];
        assert_eq!(
            toggled(&columns, Column::Owner),
            [Column::Name, Column::Size, Column::Owner]
        );
        assert_eq!(toggled(&columns, Column::Size), [Column::Name]);
        assert_eq!(toggled(&columns, Column::Name), columns);
        assert_eq!(
            toggled(&columns, Column::Icon),
            [Column::Icon, Column::Name, Column::Size]
        );
    }

    #[test]
    fn a_press_is_a_click_until_the_pointer_has_moved_a_few_pixels() {
        let spans = spans();
        let mut press =
            HeaderPress::pressed(PanelSide::Left, Column::Size, Point::new(320.0, 10.0));
        press.moved_to(Point::new(322.0, 10.0), &spans, 1001.0);
        assert_eq!(press.phase, HeaderPhase::Pressed);
        assert_eq!(press.view(), None);

        press.moved_to(Point::new(100.0, 10.0), &spans, 1001.0);
        let HeaderPhase::Reordering {
            grab_offset,
            drop_before,
            before,
            ..
        } = press.phase
        else {
            panic!("expected a reorder, got {:?}", press.phase);
        };
        assert_eq!(grab_offset, 20.0);
        assert_eq!((drop_before, before), (0, Some(Column::Name)));
        assert!(matches!(press.view(), Some(HeaderDrag::Reorder { .. })));
    }

    #[test]
    fn the_right_panel_counts_from_its_own_edge() {
        let spans = spans();
        let window = 1001.0;
        let left = panel_left(PanelSide::Right, window);
        assert_eq!(left, 501.0);
        let mut press = HeaderPress::pressed(
            PanelSide::Right,
            Column::Size,
            Point::new(left + 320.0, 10.0),
        );
        press.moved_to(Point::new(left + 450.0, 10.0), &spans, window);
        let Some(HeaderDrag::Reorder { drop_before, .. }) = press.view() else {
            panic!("expected a reorder");
        };
        assert_eq!(drop_before, 3);
    }

    #[test]
    fn resizing_follows_the_pointer_within_bounds() {
        let spans = spans();
        let mut press = HeaderPress::resizing(
            PanelSide::Left,
            Column::Size,
            Point::new(400.0, 10.0),
            100.0,
        );
        press.moved_to(Point::new(450.0, 10.0), &spans, 1001.0);
        assert_eq!(
            press.view(),
            Some(HeaderDrag::Resize {
                column: Column::Size,
                width: 150.0
            })
        );
        press.moved_to(Point::new(0.0, 10.0), &spans, 1001.0);
        assert_eq!(
            press.view(),
            Some(HeaderDrag::Resize {
                column: Column::Size,
                width: MIN_COLUMN_WIDTH
            })
        );
    }
}
