//! Top bar: app name and keyboard shortcut hints.
// reason: bounds in the signature are the short form this codebase uses
#![allow(clippy::inline_trait_bounds)]

use iced::widget::{container, row, text, Row};
use iced::{alignment, Element, Font, Length};

use super::layout::HEADER_HEIGHT;
use super::theme::{self, colors, font_size, spacing};

/// Renders the header. `shortcuts` is a list of `(key, description)` pairs.
pub fn view<'a, M: 'a>(app_name: &'a str, shortcuts: &'a [(String, &'a str)]) -> Element<'a, M> {
    let bold = Font {
        weight: iced::font::Weight::Bold,
        ..Font::MONOSPACE
    };

    let hints = Row::with_children(shortcuts.iter().map(|(key, desc)| {
        row![
            text(key.as_str())
                .size(font_size::HEADER)
                .font(bold)
                .color(colors::ACCENT),
            text(format!("={desc}"))
                .size(font_size::HEADER)
                .color(colors::DIR_COLOR),
        ]
        .into()
    }))
    .spacing(18);

    container(
        row![
            text(app_name).size(font_size::HEADER).font(bold),
            container(hints)
                .width(Length::Fill)
                .align_x(alignment::Horizontal::Right),
        ]
        .align_y(alignment::Vertical::Center),
    )
    .padding([0.0, spacing::CELL_PADDING_X * 1.5])
    .height(HEADER_HEIGHT)
    .width(Length::Fill)
    .align_y(alignment::Vertical::Center)
    .style(theme::header)
    .into()
}
