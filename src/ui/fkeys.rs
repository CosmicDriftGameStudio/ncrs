//! The function key hints at the bottom of the window: a slim row of key caps
//! with their labels. A key nothing is bound to shows no hint.
//!
//! The hints are clipped, not wrapped, when the window is narrow.

use iced::widget::{column, container, row, text, Row};
use iced::{alignment, Element, Length};

use super::layout::FKEYBAR_HEIGHT;
use super::theme::{self, font_size, spacing};
use crate::keymap::FUNCTION_KEY_COUNT;
use crate::palette::Palette;

const HINT_GAP: f32 = 14.0;
const CAP_GAP: f32 = 4.0;
const ROW_PADDING_X: f32 = 10.0;

pub fn view<'a, M: 'a>(
    labels: &[Option<&'a str>; FUNCTION_KEY_COUNT],
    palette: &Palette,
) -> Element<'a, M> {
    let hints = labels
        .iter()
        .enumerate()
        .filter_map(|(index, slot)| slot.map(|label| (index, label)))
        .map(|(index, label)| {
            let cap = container(
                text((index + 1).to_string())
                    .size(font_size::KEY_CAP)
                    .wrapping(text::Wrapping::None),
            )
            .padding([0.0, spacing::XS - 1.0])
            .style(theme::key_cap(palette, false));
            let hint: Element<'a, M> = row![
                cap,
                text(label)
                    .size(font_size::SMALL)
                    .color(palette.dim_text)
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
