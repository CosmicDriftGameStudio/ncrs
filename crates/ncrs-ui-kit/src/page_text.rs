//! Single-line texts of the full-window pages (System): a line in a chosen
//! size and color, and the accent heading of a block.

use iced::widget::text::Wrapping;
use iced::widget::{container, text, Container};
use iced::{Color, Element};

use crate::palette::Palette;
use crate::theme::{self, font_size};

/// A line that never wraps; the caller has cut it to fit.
pub fn page_line<'a, M: 'a>(content: impl Into<String>, size: f32, color: Color) -> Element<'a, M> {
    text(content.into())
        .size(size)
        .color(color)
        .wrapping(Wrapping::None)
        .into()
}

/// The heading of a block of a page, in the accent color.
pub fn page_heading<'a, M: 'a>(content: &str, palette: &Palette) -> Element<'a, M> {
    page_line(content, font_size::ROW, palette.accent)
}

/// A region of a full-window page on the darker surface of the bars, so the
/// eye stays on the region that has the plain background.
pub fn page_band<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    padding: f32,
    palette: &Palette,
) -> Container<'a, M> {
    container(content)
        .padding(padding)
        .clip(true)
        .style(theme::statusbar(palette))
}
