//! Form rows, fields and toggles.
use iced::widget::text::LineHeight;
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, text, Column, Row, Space};
use iced::{alignment, Color, Element, Length, Pixels};

use crate::kit::{self, Check};
use crate::palette::Palette;
use crate::theme;

use super::*;

// ---------------------------------------------------------------------
// Forms
// ---------------------------------------------------------------------

/// A label of 160 px, then the control and under it a hint.
pub fn form_row<'a, M: Clone + 'a>(
    label: &str,
    control: Element<'a, M>,
    hint: Option<Element<'a, M>>,
    palette: &Palette,
) -> Element<'a, M> {
    let mut right = Column::new()
        .push(
            row![Space::new().height(CONTROL_HEIGHT), control].align_y(alignment::Vertical::Center),
        )
        .spacing(GAP_XS)
        .width(Length::Fill);
    if let Some(extra) = hint {
        right = right.push(extra);
    }
    row![
        container(body(label.to_owned(), palette.dim_text))
            .width(LABEL_WIDTH)
            .height(CONTROL_HEIGHT)
            .align_y(alignment::Vertical::Center),
        right,
    ]
    .spacing(GAP_M)
    .into()
}

/// The title of a group of rows, in capitals over a line.
pub fn section<'a, M: 'a>(title: &str, palette: &Palette) -> Element<'a, M> {
    column![
        Space::new().height(GAP_S),
        container(
            column![
                container(
                    text(title.to_uppercase())
                        .size(TEXT_SMALL)
                        .font(semibold())
                        .color(palette.dim_text)
                        .wrapping(Wrapping::None),
                )
                .height(Length::Fill)
                .align_y(alignment::Vertical::Bottom),
                Space::new().height(GAP_XS),
                theme::horizontal_rule(palette.border_inactive),
            ]
            .width(Length::Fill),
        )
        .height(ROW_ONE_LINE),
    ]
    .into()
}

/// A value of a row: text of 14 px in the text colour.
pub fn value<'a, M: 'a>(
    content: impl iced::widget::text::IntoFragment<'a>,
    palette: &Palette,
) -> Element<'a, M> {
    body(content, palette.text)
}

/// [`value`] that wraps where the line is full.
pub fn sentence<'a, M: 'a>(
    content: impl iced::widget::text::IntoFragment<'a>,
    palette: &Palette,
) -> Element<'a, M> {
    text(content)
        .size(TEXT_BODY)
        .line_height(LineHeight::Absolute(Pixels(BODY_LINE)))
        .color(palette.text)
        .wrapping(Wrapping::Word)
        .into()
}

/// [`value`] in the dimmed colour.
pub fn value_dim<'a, M: 'a>(
    content: impl iced::widget::text::IntoFragment<'a>,
    palette: &Palette,
) -> Element<'a, M> {
    body(content, palette.dim_text)
}

/// [`value`] in a colour of its own, for states.
pub fn value_in<'a, M: 'a>(
    content: impl iced::widget::text::IntoFragment<'a>,
    color: Color,
) -> Element<'a, M> {
    body(content, color)
}

/// The items of one control cell side by side.
pub fn cell<'a, M: Clone + 'a>(items: Vec<Element<'a, M>>) -> Element<'a, M> {
    Row::with_children(items)
        .spacing(GAP_M)
        .align_y(alignment::Vertical::Center)
        .into()
}

/// A note of 12 px, dimmed.
pub fn hint<'a, M: 'a>(
    content: impl iced::widget::text::IntoFragment<'a>,
    palette: &Palette,
) -> Element<'a, M> {
    text(content)
        .size(TEXT_SMALL)
        .line_height(LineHeight::Absolute(Pixels(SMALL_LINE)))
        .color(palette.dim_text)
        .wrapping(Wrapping::Word)
        .into()
}

/// A problem: text in the error colour behind a bar of 2 px.
pub fn error_line<'a, M: 'a>(
    content: impl iced::widget::text::IntoFragment<'a>,
    palette: &Palette,
) -> Element<'a, M> {
    row![
        bar(palette.error),
        text(content)
            .size(TEXT_SMALL)
            .line_height(LineHeight::Absolute(Pixels(SMALL_LINE)))
            .color(palette.error)
            .wrapping(Wrapping::Word),
    ]
    .spacing(GAP_S)
    // The bar fills its height, which would otherwise make the row take all free room.
    .height(Length::Shrink)
    .into()
}

/// Text in the monospace font.
pub fn mono<'a, M: 'a>(
    content: impl iced::widget::text::IntoFragment<'a>,
    size: f32,
    color: Color,
) -> Element<'a, M> {
    text(content)
        .size(size)
        .font(crate::APP_FONT)
        .color(color)
        .wrapping(Wrapping::None)
        .into()
}

/// Buttons side by side of which one is on.
pub fn segmented<'a, M: Clone + 'a>(
    options: &[(&str, M)],
    selected: usize,
    palette: &Palette,
) -> Element<'a, M> {
    let buttons = options.iter().enumerate().map(|(index, (label, message))| {
        button(container(label_text(label)).center_y(Length::Fill))
            .padding([0.0, BUTTON_PAD_X])
            .height(CONTROL_HEIGHT)
            .on_press(message.clone())
            .style(theme::segment_button(palette, index == selected))
            .into()
    });
    container(Row::with_children(buttons).wrap())
        .style(theme::segment_group(palette))
        .into()
}

/// How much of the on color the field under the marked key block takes.
const MARKED_BLOCK_TINT: f32 = 0.22;

/// A recovery key on one line with one block marked: the marked block sits in
/// a frame on a tinted field in bold, the other blocks and the dashes stay dim.
pub fn key_with_marked_block<'a, M: 'a>(
    key_text: &str,
    marked_block: usize,
    palette: &Palette,
) -> Element<'a, M> {
    let (on, _) = palette.primary_button();
    let spans: Vec<iced::widget::text::Span<'a, (), iced::Font>> =
        key_parts(key_text, marked_block)
            .into_iter()
            .map(|(part, marked)| {
                let piece = iced::widget::span(part);
                if marked {
                    piece
                        .color(palette.text)
                        .font(crate::APP_FONT_BOLD)
                        .background(on.scale_alpha(MARKED_BLOCK_TINT))
                        .border(iced::Border {
                            color: on,
                            width: 1.0,
                            radius: theme::radius::CORNER.into(),
                        })
                        .padding([0.0, theme::spacing::XS])
                } else {
                    piece.color(palette.dim_text)
                }
            })
            .collect();
    iced::widget::rich_text(spans)
        .font(crate::APP_FONT)
        .size(theme::font_size::BODY)
        .wrapping(Wrapping::None)
        .into()
}

/// The key split into its blocks and the dashes between them, each part with
/// whether it is the marked block.
fn key_parts(key_text: &str, marked_block: usize) -> Vec<(String, bool)> {
    let mut parts = Vec::new();
    for (index, block) in key_text.split('-').enumerate() {
        if index > 0 {
            parts.push(("-".to_owned(), false));
        }
        parts.push((block.to_owned(), index == marked_block));
    }
    parts
}

// reason: a failing assertion is the signal in a test
#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod key_tests {
    use super::key_parts;

    #[test]
    fn only_the_four_characters_of_the_requested_block_are_marked() {
        let key = "AAAA-BBBB-CCCC-DDDD";
        for block in 0..4 {
            let parts = key_parts(key, block);
            let texts: Vec<&str> = parts.iter().map(|(part, _)| part.as_str()).collect();
            assert_eq!(texts, ["AAAA", "-", "BBBB", "-", "CCCC", "-", "DDDD"]);
            let marked: Vec<&str> = parts
                .iter()
                .filter(|(_, marked)| *marked)
                .map(|(part, _)| part.as_str())
                .collect();
            assert_eq!(marked, [key.split('-').nth(block).unwrap()]);
        }
    }

    #[test]
    fn the_parts_join_to_the_key_unchanged() {
        let key = "H7QM-3IEN-VGN6-F5A2";
        let joined: String = key_parts(key, 2)
            .into_iter()
            .map(|(part, _)| part)
            .collect();
        assert_eq!(joined, key);
    }
}

/// A text field: the kit's, so a field looks the same on every page.
pub fn text_field<'a, M: Clone + 'a>(
    placeholder: &str,
    value: &str,
    id: &'static str,
    on_input: Option<impl Fn(String) -> M + 'a>,
    on_submit: M,
    palette: &Palette,
) -> Element<'a, M> {
    kit::text_field(placeholder, value, id, on_input, on_submit, palette)
}

/// [`text_field`] in the monospace font.
pub fn mono_text_field<'a, M: Clone + 'a>(
    placeholder: &str,
    value: &str,
    id: &'static str,
    on_input: Option<impl Fn(String) -> M + 'a>,
    on_submit: M,
    palette: &Palette,
) -> Element<'a, M> {
    kit::mono_text_field(placeholder, value, id, on_input, on_submit, palette)
}

/// A checkbox with its label: the kit's.
pub fn checkbox<'a, M: Clone + 'a>(
    state: Check,
    on_toggle: Option<M>,
    label: Option<&str>,
    palette: &Palette,
) -> Element<'a, M> {
    kit::labelled_checkbox(state, on_toggle, label, palette)
}
