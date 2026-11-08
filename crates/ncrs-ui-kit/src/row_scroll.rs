//! Keeps the row the keyboard is on fully in view of a scrollable page.

use iced::advanced::widget::operation::{scrollable, Operation, Outcome};
use iced::advanced::widget::Id;
use iced::widget::scrollable::AbsoluteOffset;
use iced::{Rectangle, Task, Vector};

use crate::theme::spacing;

/// The id of the container around the row that has the keyboard focus.
pub const FOCUSED_ROW_ID: &str = "settings-focused-row";

/// How far the scroll has to move so that `row` (in the content's own
/// coordinates) lies within the `viewport_height` high window at `scrolled`;
/// zero when it does.
pub fn scroll_delta(scrolled: f32, viewport_height: f32, row_top: f32, row_height: f32) -> f32 {
    let row_bottom = row_top + row_height;
    if row_top - spacing::M < scrolled {
        // Rows near the top bring the page heading back with them.
        if row_bottom + spacing::M <= viewport_height {
            -scrolled
        } else {
            row_top - spacing::M - scrolled
        }
    } else if row_bottom + spacing::M > scrolled + viewport_height {
        // A row taller than the window shows its top.
        (row_bottom + spacing::M - viewport_height - scrolled).min(row_top - spacing::M - scrolled)
    } else {
        0.0
    }
}

struct Measure {
    scroll: Id,
    row: Id,
    viewport: Option<(Rectangle, Rectangle, Vector)>,
    row_bounds: Option<Rectangle>,
}

impl Operation<f32> for Measure {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<f32>)) {
        operate(self);
    }

    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        if id == Some(&self.row) {
            self.row_bounds = Some(bounds);
        }
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        content_bounds: Rectangle,
        translation: Vector,
        _state: &mut dyn scrollable::Scrollable,
    ) {
        if id == Some(&self.scroll) {
            self.viewport = Some((bounds, content_bounds, translation));
        }
    }

    fn finish(&self) -> Outcome<f32> {
        let delta = match (self.viewport, self.row_bounds) {
            (Some((viewport, content, translation)), Some(row)) => scroll_delta(
                translation.y,
                viewport.height,
                row.y - content.y,
                row.height,
            ),
            _ => 0.0,
        };
        Outcome::Some(delta)
    }
}

/// Scrolls the scrollable `scroll_id` just far enough that the container
/// with [`FOCUSED_ROW_ID`] is wholly in view.
pub fn focused_row_into_view<T: Send + 'static>(scroll_id: &'static str) -> Task<T> {
    let measure = Measure {
        scroll: Id::new(scroll_id),
        row: Id::new(FOCUSED_ROW_ID),
        viewport: None,
        row_bounds: None,
    };
    iced::advanced::widget::operate(measure).then(move |delta| {
        if delta == 0.0 {
            Task::none()
        } else {
            iced::advanced::widget::operate(scrollable::scroll_by(
                Id::new(scroll_id),
                AbsoluteOffset { x: 0.0, y: delta },
            ))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_in_view_does_not_scroll() {
        assert_eq!(scroll_delta(100.0, 400.0, 200.0, 40.0), 0.0);
    }

    #[test]
    fn a_row_below_the_window_scrolls_down_until_its_bottom_shows() {
        let delta = scroll_delta(0.0, 400.0, 500.0, 40.0);
        assert!(delta > 0.0);
        assert!(500.0 + 40.0 <= 400.0 + delta);
    }

    #[test]
    fn a_row_above_the_window_scrolls_up_until_its_top_shows() {
        let delta = scroll_delta(300.0, 400.0, 100.0, 40.0);
        assert!(delta < 0.0);
        assert!(100.0 >= 300.0 + delta);
    }

    #[test]
    fn a_row_taller_than_the_window_shows_its_top() {
        let delta = scroll_delta(0.0, 100.0, 500.0, 300.0);
        assert!(500.0 >= delta);
    }
}
