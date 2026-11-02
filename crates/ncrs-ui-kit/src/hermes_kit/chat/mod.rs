//! The Hermes chat: a messenger column with bubbles, chips and cards. Every
//! piece takes what it shows; the colours are roles of the palette.

mod bubble;
mod cards;
mod input;

pub use bubble::*;
pub use cards::*;
pub use input::*;

use iced::widget::{column, container, scrollable, Space};
use iced::{alignment, Element, Length};

use crate::palette::Palette;
use crate::theme;

use super::*;

/// The widest the conversation gets; the column is centred in a wider window.
pub const CHAT_COLUMN_WIDTH: f32 = 780.0;
const CHAT_HEADER_HEIGHT: f32 = 56.0;
const THREAD_PADDING: f32 = 20.0;
const FOOTER_PADDING: f32 = 14.0;

fn centred_column<'a, M: 'a>(content: Element<'a, M>) -> Element<'a, M> {
    container(container(content).max_width(CHAT_COLUMN_WIDTH))
        .center_x(Length::Fill)
        .into()
}

/// The whole chat: `header`, the scrolling `thread` that stays at its end when
/// it grows, and the footer with the `chips` of the last question and the
/// `input`.
pub fn chat_page<'a, M: Clone + 'a>(
    header: Element<'a, M>,
    thread: Vec<Element<'a, M>>,
    chips: Option<Element<'a, M>>,
    input: Element<'a, M>,
    scroll_id: &'static str,
    palette: &Palette,
) -> Element<'a, M> {
    let messages = scrollable(centred_column(
        iced::widget::Column::with_children(thread)
            .spacing(GAP_M)
            .padding(THREAD_PADDING)
            .width(Length::Fill)
            .into(),
    ))
    .id(scroll_id)
    .anchor_bottom()
    .width(Length::Fill)
    .height(Length::Fill);
    let mut footer = iced::widget::Column::new().spacing(GAP_S);
    if let Some(row) = chips {
        footer = footer.push(row);
    }
    footer = footer.push(input);
    let colors = *palette;
    container(column![
        container(centred_column(header))
            .width(Length::Fill)
            .height(CHAT_HEADER_HEIGHT)
            .align_y(alignment::Vertical::Center),
        theme::horizontal_rule(colors.border_inactive),
        messages,
        container(centred_column(
            container(footer).padding(FOOTER_PADDING).into()
        ))
        .width(Length::Fill),
        Space::new().height(GAP_XS),
    ])
    .width(Length::Fill)
    .height(Length::Fill)
    .style(theme::panel(palette))
    .into()
}
