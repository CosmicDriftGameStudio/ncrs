//! Single-line texts of the full-window pages (System): a line in a chosen
//! size and color, and the heading of a block.

use iced::widget::text::Wrapping;
use iced::widget::{container, text, Container};
use iced::{Color, Element, Length};

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

/// A line that wraps at words and takes the width it is given, for text that must give way to its neighbours.
pub fn page_wrapped<'a, M: 'a>(
    content: impl Into<String>,
    size: f32,
    color: Color,
) -> Element<'a, M> {
    text(content.into())
        .size(size)
        .color(color)
        .width(Length::Fill)
        .wrapping(Wrapping::Word)
        .into()
}

/// A small bold label in capitals, in the given color.
pub fn caps_label<'a, M: 'a>(content: &str, color: Color) -> Element<'a, M> {
    text(content.to_uppercase())
        .size(font_size::SMALL)
        .font(crate::APP_FONT_BOLD)
        .color(color)
        .into()
}

/// A bold text in a chosen size and color.
pub fn bold_line<'a, M: 'a>(content: impl Into<String>, size: f32, color: Color) -> Element<'a, M> {
    text(content.into())
        .size(size)
        .font(crate::APP_FONT_BOLD)
        .color(color)
        .into()
}

/// The heading of a block of a page: bold, in the text color.
pub fn page_heading<'a, M: 'a>(content: &str, palette: &Palette) -> Element<'a, M> {
    text(content.to_owned())
        .size(font_size::ROW)
        .font(crate::APP_FONT_BOLD)
        .color(palette.text)
        .wrapping(Wrapping::None)
        .into()
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

/// A text that wraps at the width of the page and keeps its line breaks and
/// spaces: a file shown as it is on disk.
pub fn page_source<'a, M: 'a>(content: &str, palette: &Palette) -> Element<'a, M> {
    text(content.to_owned())
        .size(font_size::ROW)
        .font(crate::APP_FONT)
        .color(palette.text)
        .wrapping(Wrapping::WordOrGlyph)
        .into()
}

/// [`page_heading`] that wraps at the width of its block instead of running past it.
pub fn page_heading_wrapped<'a, M: 'a>(content: &str, palette: &Palette) -> Element<'a, M> {
    text(content.to_owned())
        .size(font_size::ROW)
        .font(crate::APP_FONT_BOLD)
        .color(palette.text)
        .wrapping(Wrapping::WordOrGlyph)
        .width(Length::Fill)
        .into()
}

/// A line of a block that wraps at the width of the block.
pub fn page_paragraph<'a, M: 'a>(
    content: impl Into<String>,
    size: f32,
    color: Color,
) -> Element<'a, M> {
    text(content.into())
        .size(size)
        .color(color)
        .wrapping(Wrapping::WordOrGlyph)
        .width(Length::Fill)
        .into()
}
