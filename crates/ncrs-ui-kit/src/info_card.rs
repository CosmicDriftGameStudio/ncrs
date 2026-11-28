//! A card of a page that explains something: a bold title, lines under it, a bordered box.

use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, text, Space};
use iced::{alignment, Color, Element, Length};

use crate::page_text::page_paragraph;
use crate::palette::Palette;
use crate::theme::{self, font_size, size, spacing};
use crate::APP_FONT_BOLD;

/// The card: `title` with an optional colour swatch before it, then the lines.
pub fn info_card<'a, M: 'a>(
    title: &str,
    swatch: Option<Color>,
    lines: Vec<Element<'a, M>>,
    palette: &Palette,
) -> Element<'a, M> {
    let mut head = row![]
        .spacing(spacing::M)
        .align_y(alignment::Vertical::Center);
    if let Some(color) = swatch {
        head = head.push(
            container(Space::new())
                .width(spacing::M)
                .height(spacing::M)
                .style(theme::line(color)),
        );
    }
    head = head.push(
        text(title.to_owned())
            .size(font_size::BODY)
            .font(APP_FONT_BOLD)
            .wrapping(Wrapping::None),
    );
    container(column![head, column(lines).spacing(spacing::M)].spacing(spacing::L))
        .padding(spacing::XL)
        .width(Length::Fill)
        .style(theme::outlined(palette))
        .into()
}

/// A line of a card with a dot before it.
pub fn dot_line<'a, M: 'a>(content: Element<'a, M>, palette: &Palette) -> Element<'a, M> {
    row![
        text("\u{00b7}")
            .size(font_size::BODY)
            .color(palette.dim_text)
            .wrapping(Wrapping::None),
        container(content).width(Length::Fill),
    ]
    .spacing(spacing::M)
    .into()
}

/// Running text of a card.
pub fn card_text<'a, M: 'a>(content: impl Into<String>, palette: &Palette) -> Element<'a, M> {
    page_paragraph(content, font_size::BODY, palette.text)
}

/// A dim note of a card.
pub fn card_note<'a, M: 'a>(content: impl Into<String>, palette: &Palette) -> Element<'a, M> {
    page_paragraph(content, font_size::SMALL, palette.dim_text)
}

/// A key on its cap with what it does.
pub fn key_row<'a, M: 'a>(key: &str, words: &str, palette: &Palette) -> Element<'a, M> {
    row![
        container(
            container(
                text(key.to_owned())
                    .size(font_size::SMALL)
                    .wrapping(Wrapping::None)
            )
            .padding([0.0, spacing::XS])
            .style(theme::key_cap(palette, false))
        )
        .width(size::KEY_COLUMN),
        text(words.to_owned())
            .size(font_size::BODY)
            .wrapping(Wrapping::None),
    ]
    .spacing(spacing::L)
    .align_y(alignment::Vertical::Center)
    .into()
}
