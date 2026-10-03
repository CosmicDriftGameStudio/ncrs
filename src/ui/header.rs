//! Top bar: the app name. The key hints live on the function key bar.

use iced::widget::{container, row, text, Space};
use iced::{alignment, Element, Font, Length};

use super::layout::HEADER_HEIGHT;
use super::theme::{self, font_size, spacing};

/// The right end is the notice's: a confirmation that goes away by itself.
pub fn view<'a, M: 'a>(app_name: &'a str, notice: Option<&'a str>) -> Element<'a, M> {
    let bold = Font {
        weight: iced::font::Weight::Bold,
        ..Font::MONOSPACE
    };

    let confirmation: Element<'a, M> = match notice {
        Some(shown) => text(format!("\u{2713} {shown}"))
            .size(font_size::HEADER)
            .color(theme::colors::SUCCESS)
            .wrapping(text::Wrapping::None)
            .into(),
        None => Space::new().into(),
    };

    container(row![
        text(app_name).size(font_size::HEADER).font(bold),
        Space::new().width(Length::Fill),
        confirmation,
    ])
    .padding([0.0, spacing::CELL_PADDING_X * 1.5])
    .height(HEADER_HEIGHT)
    .width(Length::Fill)
    .align_y(alignment::Vertical::Center)
    .style(theme::header)
    .into()
}
