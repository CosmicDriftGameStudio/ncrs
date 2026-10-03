//! The function key bar at the bottom of the window, as in Norton Commander:
//! ten equal slots, each a digit and a coloured label box. A key nothing is
//! bound to gets an empty box.
//!
//! Labels are clipped, not wrapped, when the window is narrow; the digits are
//! outside the clipped part and always stay visible.

use iced::widget::{container, row, text, Row};
use iced::{alignment, Element, Length};

use super::layout::FKEYBAR_HEIGHT;
use super::theme::{self, font_size, spacing};
use crate::keymap::FUNCTION_KEY_COUNT;
use crate::palette::Palette;

pub fn view<'a, M: 'a>(
    labels: &[Option<&'a str>; FUNCTION_KEY_COUNT],
    palette: &Palette,
) -> Element<'a, M> {
    let slots = labels.iter().enumerate().map(|(index, label)| {
        let number = text((index + 1).to_string())
            .size(font_size::STATUS)
            .color(palette.fkey_number)
            .wrapping(text::Wrapping::None);
        let label_box = container(
            text(label.unwrap_or(""))
                .size(font_size::STATUS)
                .wrapping(text::Wrapping::None),
        )
        .padding([0.0, spacing::CELL_PADDING_X / 2.0])
        .width(Length::Fill)
        .height(Length::Fill)
        .clip(true)
        .align_y(alignment::Vertical::Center)
        .style(theme::fkey_label(palette));

        // Equal portions: the ten slots share the full width evenly.
        row![number, label_box]
            .spacing(2)
            .width(Length::FillPortion(1))
            .into()
    });

    container(Row::with_children(slots).spacing(spacing::BORDER_WIDTH))
        .height(FKEYBAR_HEIGHT)
        .width(Length::Fill)
        .into()
}
