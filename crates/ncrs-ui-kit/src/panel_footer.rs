//! The line under the rows of a file panel: the totals of what it shows.

use iced::widget::text::Wrapping;
use iced::widget::{container, text};
use iced::{alignment, Element, Length};

use crate::palette::Palette;
use crate::theme::{font_size, size, spacing};

pub fn panel_footer<'a, M: 'a>(line: String, palette: &Palette) -> Element<'a, M> {
    container(
        text(line)
            .size(font_size::SMALL)
            .color(palette.dim_text)
            .wrapping(Wrapping::None),
    )
    .padding([0.0, spacing::L])
    .width(Length::Fill)
    .height(Length::Fixed(size::PANEL_FOOTER))
    .align_y(alignment::Vertical::Center)
    .into()
}
