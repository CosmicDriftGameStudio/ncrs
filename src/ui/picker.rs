//! The box Quick Open and the command palette share: a title, a text field, a
//! status line and a list of rows with the matched characters marked.

use std::ops::Range;

use iced::widget::text::{Span, Wrapping};
use iced::widget::{button, column, container, rich_text, span, text, text_input, Column};
use iced::{Element, Length};

use super::layout::ROW_HEIGHT;
use super::theme::{self, font_size, spacing};
use crate::messages::Message;
use crate::palette::Palette;

/// Wider than the other dialogs: it shows paths.
const WIDTH: f32 = 560.0;

/// The box around the field and the rows, which get room for `row_count`.
pub fn frame<'a>(
    title: String,
    field: Element<'a, Message>,
    status: String,
    rows: impl Iterator<Item = Element<'a, Message>>,
    row_count: usize,
    palette: &Palette,
) -> Element<'a, Message> {
    let body = column![
        text(title).size(font_size::TITLE).color(palette.accent),
        field,
        text(status)
            .size(font_size::STATUS)
            .color(palette.dim_text)
            .wrapping(Wrapping::None),
        container(Column::with_children(rows))
            .height(ROW_HEIGHT * row_count as f32)
            .clip(true),
    ]
    .spacing(spacing::SECTION_GAP);

    container(body)
        .padding(spacing::OUTER_PADDING * 2.0)
        .width(WIDTH)
        .style(theme::dialog(palette))
        .into()
}

pub fn query_field<'a>(
    placeholder: &str,
    query: &str,
    id: &'static str,
    on_input: impl Fn(String) -> Message + 'a,
    on_submit: Message,
    palette: &Palette,
) -> Element<'a, Message> {
    text_input(placeholder, query)
        .id(id)
        .on_input(on_input)
        .on_submit(on_submit)
        .size(font_size::ROW)
        .padding([3.0, 6.0])
        .style(theme::search_input(palette))
        .width(Length::Fill)
        .into()
}

/// One row of the list; a click sends `on_press`.
pub fn list_row<'a>(
    content: Element<'a, Message>,
    on_press: Message,
    selected: bool,
    palette: &Palette,
) -> Element<'a, Message> {
    button(content)
        .on_press(on_press)
        .width(Length::Fill)
        .padding([0.0, spacing::CELL_PADDING_X])
        .height(ROW_HEIGHT)
        .style(theme::row(palette, false, selected, true))
        .into()
}

/// `text` cut at the matched ranges, which are drawn in the accent color.
pub fn highlighted<'a>(
    text: &'a str,
    highlight: &[Range<usize>],
    selected: bool,
    palette: &Palette,
) -> Element<'a, Message> {
    rich_text(highlight_spans(text, highlight, selected, palette))
        .size(font_size::ROW)
        .wrapping(Wrapping::None)
        .into()
}

fn highlight_spans<'a>(
    text: &'a str,
    highlight: &[Range<usize>],
    selected: bool,
    palette: &Palette,
) -> Vec<Span<'a, ()>> {
    let plain = if selected {
        palette.cursor_text
    } else {
        palette.text
    };
    let mut spans = Vec::new();
    let mut at = 0;
    for range in highlight {
        let (Some(before), Some(hit)) = (text.get(at..range.start), text.get(range.clone())) else {
            continue;
        };
        if !before.is_empty() {
            spans.push(span(before).color(plain));
        }
        spans.push(
            span(hit)
                .color(if selected { plain } else { palette.accent })
                .underline(true),
        );
        at = range.end;
    }
    if let Some(rest) = text.get(at..) {
        if !rest.is_empty() {
            spans.push(span(rest).color(plain));
        }
    }
    spans
}
