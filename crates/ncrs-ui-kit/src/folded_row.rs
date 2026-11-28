//! A row of a list that stands for rows not shown: an arrow, what they are, and the key
//! that lists them.

use iced::widget::text::Wrapping;
use iced::widget::{container, mouse_area, row, text};
use iced::{alignment, Element, Length};

use crate::kit;
use crate::palette::Palette;
use crate::theme::{font_size, size, spacing};

/// `what` is the words of the hidden rows, `key` the cap after them, `word` what the key does.
pub fn folded_row<'a, M: Clone + 'a>(
    what: &str,
    key: &str,
    word: &str,
    on_press: M,
    palette: &Palette,
) -> Element<'a, M> {
    let dim = |content: String| {
        text(content)
            .size(font_size::SMALL)
            .color(palette.dim_text)
            .wrapping(Wrapping::None)
    };
    let line = row![
        dim("\u{25b8}".to_owned()),
        dim(what.to_owned()),
        kit::key_cap(key, palette),
        dim(word.to_owned()),
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    mouse_area(
        container(line)
            .padding([0.0, spacing::XL])
            .width(Length::Fill)
            .height(size::SIZE_ROW)
            .align_y(alignment::Vertical::Center),
    )
    .on_press(on_press)
    .into()
}
