//! The bar of a running folder comparison: a title, the tallies in the colors
//! of their marks, and the controls at the right end. Looks like the
//! favourites bar above it.

use iced::widget::{column, container, row, text, Row, Space};
use iced::{alignment, Color, Element, Length};

use crate::fkeys::{HINT_GAP, ROW_PADDING_X};
use crate::palette::Palette;
use crate::theme::{self, font_size, size};

pub const COMPARE_BAR_HEIGHT: f32 = size::COMPARE_BAR;

/// One figure of the bar, like `3 only left`, in `color`.
#[derive(Debug, Clone)]
pub struct Tally {
    pub text: String,
    pub color: Color,
}

/// `controls` sit at the right end, in the order given.
pub fn compare_bar<'a, M: Clone + 'a>(
    title: &str,
    tallies: Vec<Tally>,
    controls: Vec<Element<'a, M>>,
    palette: &Palette,
) -> Element<'a, M> {
    let small = |content: String, color: Color| {
        text(content)
            .size(font_size::SMALL)
            .color(color)
            .wrapping(text::Wrapping::None)
    };
    let figures = tallies
        .into_iter()
        .map(|tally| small(tally.text, tally.color).into());
    let line = row![
        small(title.to_owned(), palette.text),
        Row::with_children(figures).spacing(HINT_GAP),
        Space::new().width(Length::Fill),
        Row::with_children(controls)
            .spacing(HINT_GAP)
            .align_y(alignment::Vertical::Center),
    ]
    .spacing(HINT_GAP)
    .align_y(alignment::Vertical::Center);
    container(column![
        container(line)
            .padding([0.0, ROW_PADDING_X])
            .width(Length::Fill)
            .height(Length::Fill)
            .clip(true)
            .align_y(alignment::Vertical::Center),
        theme::horizontal_rule(palette.border_inactive),
    ])
    .height(COMPARE_BAR_HEIGHT)
    .width(Length::Fill)
    .style(theme::statusbar(palette))
    .into()
}
