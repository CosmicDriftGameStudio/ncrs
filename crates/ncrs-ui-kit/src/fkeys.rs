//! The function key hints at the bottom of the window: a slim row of key caps
//! with their labels. A key nothing is bound to shows no hint.
//!
//! The hints are clipped, not wrapped, when the window is narrow.

use iced::widget::{column, container, row, text, Row};
use iced::{alignment, Color, Element, Length};

use super::layout::{FKEYBAR_HEIGHT, FUNCTION_KEY_COUNT};
use super::theme::{self, font_size, spacing};
use crate::palette::Palette;

pub const HINT_GAP: f32 = 14.0;
pub const CAP_GAP: f32 = 4.0;
pub const ROW_PADDING_X: f32 = spacing::L;
const DIMMED_ALPHA: f32 = 0.4;

pub fn view<'a, M: 'a>(
    labels: &[Option<&'a str>; FUNCTION_KEY_COUNT],
    palette: &Palette,
) -> Element<'a, M> {
    view_marked(labels, None, palette)
}

/// [`view`] with the keys at the slots set in `dimmed` drawn faint: the
/// action is there but refused in this panel.
pub fn view_dimmed<'a, M: 'a>(
    labels: &[Option<&'a str>; FUNCTION_KEY_COUNT],
    dimmed: &[bool; FUNCTION_KEY_COUNT],
    palette: &Palette,
) -> Element<'a, M> {
    bar(labels, None, dimmed, palette)
}

/// [`view`] with the key at slot `marked` (0 for F1) drawn on the cursor
/// colour, for the key whose screen is up.
pub fn view_marked<'a, M: 'a>(
    labels: &[Option<&'a str>; FUNCTION_KEY_COUNT],
    marked: Option<usize>,
    palette: &Palette,
) -> Element<'a, M> {
    bar(labels, marked, &[false; FUNCTION_KEY_COUNT], palette)
}

fn bar<'a, M: 'a>(
    labels: &[Option<&'a str>; FUNCTION_KEY_COUNT],
    marked: Option<usize>,
    dimmed: &[bool; FUNCTION_KEY_COUNT],
    palette: &Palette,
) -> Element<'a, M> {
    let hints = labels
        .iter()
        .enumerate()
        .filter_map(|(index, slot)| slot.map(|label| (index, label)))
        .map(|(index, label)| {
            let is_dimmed = dimmed.get(index).copied().unwrap_or(false);
            let cap = numbered_cap(
                (index + 1).to_string(),
                marked == Some(index),
                is_dimmed,
                palette,
            );
            let hint: Element<'a, M> = row![
                cap,
                text(label)
                    .size(font_size::SMALL)
                    .color(if marked == Some(index) {
                        palette.text
                    } else if is_dimmed {
                        fainter(palette.dim_text)
                    } else {
                        palette.dim_text
                    })
                    .wrapping(text::Wrapping::None)
            ]
            .spacing(CAP_GAP)
            .align_y(alignment::Vertical::Center)
            .into();
            hint
        });

    column![
        theme::horizontal_rule(palette.border_inactive),
        container(Row::with_children(hints).spacing(HINT_GAP))
            .padding([0.0, ROW_PADDING_X])
            .width(Length::Fill)
            .height(Length::Fill)
            .clip(true)
            .align_y(alignment::Vertical::Center),
    ]
    .height(FKEYBAR_HEIGHT)
    .into()
}

/// The key cap of a hint: filled on the cursor colour when `marked`, faint
/// when `dimmed`. The favourites bar draws its numbers the same way.
pub fn numbered_cap<'a, M: 'a>(
    number: String,
    marked: bool,
    dimmed: bool,
    palette: &Palette,
) -> Element<'a, M> {
    container(
        text(number)
            .size(font_size::KEY_CAP)
            .wrapping(text::Wrapping::None),
    )
    .padding([0.0, spacing::XS - 1.0])
    .style(marked_cap(palette, marked, dimmed))
    .into()
}

fn marked_cap(
    palette: &Palette,
    marked: bool,
    dimmed: bool,
) -> impl Fn(&iced::Theme) -> container::Style {
    let plain = theme::key_cap(palette, marked);
    let fill = palette.cursor;
    move |theme| {
        let mut style = plain(theme);
        if marked {
            style.background = Some(fill.into());
        } else if dimmed {
            style.text_color = style.text_color.map(fainter);
            style.border.color = fainter(style.border.color);
        }
        style
    }
}

fn fainter(color: Color) -> Color {
    Color {
        a: color.a * DIMMED_ALPHA,
        ..color
    }
}
