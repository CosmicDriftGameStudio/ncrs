//! Top bar: the app name. The key hints live on the function key bar.

use iced::widget::{container, text};
use iced::{alignment, Element, Font, Length};

use super::layout::HEADER_HEIGHT;
use super::theme::{self, font_size, spacing};

pub fn view<'a, M: 'a>(app_name: &'a str) -> Element<'a, M> {
    let bold = Font {
        weight: iced::font::Weight::Bold,
        ..Font::MONOSPACE
    };

    container(text(app_name).size(font_size::HEADER).font(bold))
        .padding([0.0, spacing::CELL_PADDING_X * 1.5])
        .height(HEADER_HEIGHT)
        .width(Length::Fill)
        .align_y(alignment::Vertical::Center)
        .style(theme::header)
        .into()
}
