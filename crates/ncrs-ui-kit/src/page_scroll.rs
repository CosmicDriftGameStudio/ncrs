//! A full-window page that scrolls vertically (the file viewer, the notes), and
//! the scroll steps its keys take.

use iced::advanced::widget::operation::scrollable;
use iced::advanced::widget::Id;
use iced::widget::scrollable::{AbsoluteOffset, Direction, RelativeOffset, Scrollbar};
use iced::widget::{container, scrollable as scrollable_widget, Column, Scrollable, Space};
use iced::{Element, Length, Padding, Task};

use crate::frame::{SCROLLBAR_MARGIN, SCROLLBAR_WIDTH};
use crate::theme::spacing;

/// `content` in a vertical scrollable of `id` that fills the room, set in from
/// the window edge by `PAGE_PADDING`.
pub fn page_scroll<'a, M: 'a>(id: &'static str, content: Element<'a, M>) -> Element<'a, M> {
    page_scroll_padded(id, content, Padding::from([spacing::XL, spacing::XXL]))
}

/// [`page_scroll`] with another margin around the content.
pub fn page_scroll_padded<'a, M: 'a>(
    id: &'static str,
    content: Element<'a, M>,
    padding: Padding,
) -> Element<'a, M> {
    page_scrollable(id, content, padding).into()
}

/// [`page_scroll`] that reports how far down the user has scrolled it, in
/// pixels. A scroll by task (`scroll_to`, [`scroll_page_by`]) is not reported.
pub fn page_scroll_tracked<'a, M: 'a>(
    id: &'static str,
    content: Element<'a, M>,
    on_scroll: impl Fn(f32) -> M + 'a,
) -> Element<'a, M> {
    page_scrollable(id, content, Padding::from([spacing::XL, spacing::XXL]))
        .on_scroll(move |viewport| on_scroll(viewport.absolute_offset().y))
        .into()
}

/// [`page_scroll_tracked`] with another margin around the content.
pub fn page_scroll_tracked_padded<'a, M: 'a>(
    id: &'static str,
    content: Element<'a, M>,
    padding: Padding,
    on_scroll: impl Fn(f32) -> M + 'a,
) -> Element<'a, M> {
    // The bar floats in the right margin, so a column head above the page
    // lines up with the columns whether the bar is shown or not.
    page_scrollable(id, content, padding)
        .direction(Direction::Vertical(
            Scrollbar::new()
                .width(SCROLLBAR_WIDTH)
                .scroller_width(SCROLLBAR_WIDTH)
                .margin(SCROLLBAR_MARGIN),
        ))
        .on_scroll(move |viewport| on_scroll(viewport.absolute_offset().y))
        .into()
}

/// Rows built above and below the ones in view.
const ROWS_AROUND: usize = 24;

/// The part of a long list that is built.
#[derive(Debug, Clone, Copy)]
pub struct RowWindow {
    pub count: usize,
    pub row_height: f32,
    /// How far down the list is scrolled, in pixels.
    pub offset: f32,
    /// At least the height of the list on the screen.
    pub height: f32,
}

impl RowWindow {
    fn built(self) -> std::ops::Range<usize> {
        let visible = (self.height / self.row_height).ceil() as usize;
        let top = (self.offset.max(0.0) / self.row_height) as usize;
        let top_row = top.min(self.count.saturating_sub(visible));
        top_row.saturating_sub(ROWS_AROUND)..(top_row + visible + ROWS_AROUND).min(self.count)
    }
}

/// [`page_scroll_tracked`] for a list of many rows of one height: only those
/// near the scroll position are built. Every scroll, by task too, has to end up
/// in [`RowWindow::offset`].
pub fn page_rows<'a, M: 'a>(
    id: &'static str,
    window: RowWindow,
    row: impl Fn(usize) -> Element<'a, M>,
    on_scroll: impl Fn(f32) -> M + 'a,
) -> Element<'a, M> {
    let built = window.built();
    let room = |rows: usize| Space::new().height(rows as f32 * window.row_height);
    let rows = Column::new()
        .push(room(built.start))
        .extend(built.clone().map(row))
        .push(room(window.count - built.end))
        .width(Length::Fill);
    page_scroll_tracked(id, rows.into(), on_scroll)
}

fn page_scrollable<'a, M: 'a>(
    id: &'static str,
    content: Element<'a, M>,
    padding: Padding,
) -> Scrollable<'a, M> {
    scrollable_widget(container(content).padding(padding))
        .direction(Direction::Vertical(
            Scrollbar::new()
                .width(SCROLLBAR_WIDTH)
                .scroller_width(SCROLLBAR_WIDTH)
                .margin(SCROLLBAR_MARGIN)
                .spacing(0.0),
        ))
        .id(id)
        .width(Length::Fill)
        .height(Length::Fill)
}

/// Moves the page `id` by `delta` pixels, down when positive.
pub fn scroll_page_by<T: Send + 'static>(id: &'static str, delta: f32) -> Task<T> {
    iced::advanced::widget::operate(scrollable::scroll_by(
        Id::new(id),
        AbsoluteOffset { x: 0.0, y: delta },
    ))
}

/// Jumps to the top of the page `id`, or to its end.
pub fn scroll_page_to_edge<T: Send + 'static>(id: &'static str, end: bool) -> Task<T> {
    iced::advanced::widget::operate(scrollable::snap_to(
        Id::new(id),
        RelativeOffset {
            x: Some(0.0),
            y: Some(if end { 1.0 } else { 0.0 }),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn built(count: usize, offset: f32) -> std::ops::Range<usize> {
        RowWindow {
            count,
            row_height: 20.0,
            offset,
            height: 200.0,
        }
        .built()
    }

    #[test]
    fn only_the_rows_near_the_scroll_position_are_built() {
        assert_eq!(built(10_000, 0.0), 0..10 + ROWS_AROUND);
        let middle = built(10_000, 100_000.0);
        assert_eq!(middle, 5_000 - ROWS_AROUND..5_010 + ROWS_AROUND);
    }

    #[test]
    fn the_window_stays_inside_the_list() {
        assert_eq!(built(3, 0.0), 0..3);
        assert_eq!(built(0, 400.0), 0..0);
        assert_eq!(built(100, 1_000_000.0).end, 100);
        assert_eq!(built(100, -50.0).start, 0);
    }
}
