//! The bar of a running folder comparison: a title, the tallies in the colors
//! of their marks, and the controls at the right end. Looks like the
//! favourites bar above it. A page has it at the bottom, see [`mode_tally_bar`].

use iced::widget::{column, container, row, text, Column, Row, Space};
use iced::{alignment, Color, Element, Length};

use crate::fkeys::{HINT_GAP, ROW_PADDING_X};
use crate::page_status_bar::mode_field;
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
    let line = row![
        small(title.to_owned(), palette.text),
        figures_and_controls(tallies, controls),
    ]
    .spacing(HINT_GAP)
    .align_y(alignment::Vertical::Center);
    let line = container(line)
        .padding([0.0, ROW_PADDING_X])
        .width(Length::Fill)
        .height(Length::Fill)
        .clip(true)
        .align_y(alignment::Vertical::Center);
    bar(
        column![line, theme::horizontal_rule(palette.border_inactive)],
        palette,
    )
}

/// The same bar under a page, with the field that names the page in place of
/// the title and the line above it.
pub fn mode_tally_bar<'a, M: Clone + 'a>(
    mode: &str,
    tallies: Vec<Tally>,
    controls: Vec<Element<'a, M>>,
    palette: &Palette,
) -> Element<'a, M> {
    let rest = container(figures_and_controls(tallies, controls))
        .padding([0.0, ROW_PADDING_X])
        .width(Length::Fill)
        .clip(true);
    let line = row![mode_field(mode, palette), rest]
        .height(Length::Fill)
        .align_y(alignment::Vertical::Center);
    bar(
        column![theme::horizontal_rule(palette.border_inactive), line],
        palette,
    )
}

fn small<'a>(content: String, color: Color) -> text::Text<'a> {
    text(content)
        .size(font_size::SMALL)
        .color(color)
        .wrapping(text::Wrapping::None)
}

fn figures_and_controls<'a, M: 'a>(
    tallies: Vec<Tally>,
    controls: Vec<Element<'a, M>>,
) -> Row<'a, M> {
    let figures = tallies
        .into_iter()
        .map(|tally| small(tally.text, tally.color).into());
    row![
        Row::with_children(figures).spacing(HINT_GAP),
        Space::new().width(Length::Fill),
        Row::with_children(controls)
            .spacing(HINT_GAP)
            .align_y(alignment::Vertical::Center),
    ]
    .spacing(HINT_GAP)
    .width(Length::Fill)
    .align_y(alignment::Vertical::Center)
}

fn bar<'a, M: 'a>(lines: Column<'a, M>, palette: &Palette) -> Element<'a, M> {
    container(lines)
        .height(COMPARE_BAR_HEIGHT)
        .width(Length::Fill)
        .style(theme::statusbar(palette))
        .into()
}
