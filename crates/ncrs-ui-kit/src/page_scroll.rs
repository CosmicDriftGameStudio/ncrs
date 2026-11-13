//! A full-window page that scrolls vertically (the file viewer, the notes), and
//! the scroll steps its keys take.

use iced::advanced::widget::operation::scrollable;
use iced::advanced::widget::Id;
use iced::widget::scrollable::{AbsoluteOffset, Direction, RelativeOffset, Scrollbar};
use iced::widget::{container, scrollable as scrollable_widget};
use iced::{Element, Length, Task};

use crate::frame::{SCROLLBAR_MARGIN, SCROLLBAR_WIDTH};
use crate::theme::spacing;

/// `content` in a vertical scrollable of `id` that fills the room, set in from
/// the window edge by `PAGE_PADDING`.
pub fn page_scroll<'a, M: 'a>(id: &'static str, content: Element<'a, M>) -> Element<'a, M> {
    scrollable_widget(container(content).padding([spacing::XL, spacing::XXL]))
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
        .into()
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
