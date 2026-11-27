//! The card of the Platz answer: where the space of the drive is, as a bar,
//! the legend and a table of the biggest folders. The buttons that act on it
//! are chips of the chat, not part of the card.
use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{container, text, Column, Row};
use iced::{alignment, Background, Border, Color, Element, Length, Pixels, Theme};

use crate::color_chip::color_chip;
use crate::palette::Palette;
use crate::stacked_bar::{stacked_bar, BarSegment};
use crate::theme;

use super::*;

const CARD_PAD_Y: f32 = 8.0;
const CARD_PAD_X: f32 = 12.0;
const CARD_RADIUS: f32 = 8.0;
const LINE: f32 = 20.0;
const SIZE_COLUMN: f32 = 80.0;
const GROUP_COLUMN: f32 = 130.0;
const NAME_PORTION: u16 = 3;
const HINT_PORTION: u16 = 4;

/// One folder of the table.
#[derive(Debug, Clone, PartialEq)]
pub struct SpaceCardRow {
    pub name: String,
    /// Where it lies, under the name; empty for none.
    pub path: String,
    pub size: String,
    pub group: String,
    pub group_color: Color,
    pub hint: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpaceCardView {
    pub title: String,
    /// The drive and its size, right of the title.
    pub drive: String,
    pub bar: Vec<BarSegment>,
    /// The legend under the bar: a label with its figure and the color of its part.
    pub legend: Vec<(String, Color)>,
    /// What the drive would look like after the safe ones went.
    pub after: Option<String>,
    /// The headings of the four columns: what, size, group, hint.
    pub columns: [String; 4],
    pub rows: Vec<SpaceCardRow>,
    /// The folders that did not fit the table, in one line.
    pub also: Option<String>,
    pub footer: String,
}

fn line<'a, M: 'a>(content: &str, color: Color, bold: bool) -> Element<'a, M> {
    let label = text(content.to_owned())
        .size(TEXT_SMALL)
        .line_height(LineHeight::Absolute(Pixels(LINE)))
        .color(color)
        .wrapping(Wrapping::WordOrGlyph);
    if bold {
        label.font(semibold()).into()
    } else {
        label.into()
    }
}

fn row_of<'a, M: Clone + 'a>(row: &SpaceCardRow, palette: &Palette) -> Element<'a, M> {
    let mut name = Column::new().push(line(&row.name, palette.text, true));
    if !row.path.is_empty() {
        name = name.push(line(&row.path, palette.dim_text, false));
    }
    Row::new()
        .push(container(name).width(Length::FillPortion(NAME_PORTION)))
        .push(
            container(line(&row.size, palette.text, false))
                .width(SIZE_COLUMN)
                .align_x(alignment::Horizontal::Right),
        )
        .push(
            container(color_chip(&row.group, row.group_color, None))
                .width(GROUP_COLUMN)
                .padding([0.0, GAP_S]),
        )
        .push(
            container(line(&row.hint, palette.dim_text, false))
                .width(Length::FillPortion(HINT_PORTION)),
        )
        .spacing(GAP_S)
        .align_y(alignment::Vertical::Center)
        .into()
}

fn heading_row<'a, M: 'a>(columns: &[String; 4], palette: &Palette) -> Element<'a, M> {
    let [what, size, group, hint] = columns;
    Row::new()
        .push(
            container(line(what, palette.dim_text, true)).width(Length::FillPortion(NAME_PORTION)),
        )
        .push(
            container(line(size, palette.dim_text, true))
                .width(SIZE_COLUMN)
                .align_x(alignment::Horizontal::Right),
        )
        .push(
            container(line(group, palette.dim_text, true))
                .width(GROUP_COLUMN)
                .padding([0.0, GAP_S]),
        )
        .push(
            container(line(hint, palette.dim_text, true)).width(Length::FillPortion(HINT_PORTION)),
        )
        .spacing(GAP_S)
        .into()
}

pub fn space_card<'a, M: Clone + 'a>(view: &SpaceCardView, palette: &Palette) -> Element<'a, M> {
    let head = Row::new()
        .push(line(&view.title, palette.text, true))
        .push(
            container(line(&view.drive, palette.dim_text, false))
                .width(Length::Fill)
                .align_x(alignment::Horizontal::Right),
        )
        .spacing(GAP_S);
    let mut legend = Row::new().spacing(GAP_XS);
    for (label, color) in &view.legend {
        legend = legend.push(color_chip::<M>(label, *color, None));
    }
    let mut content = Column::new()
        .push(head)
        .push(stacked_bar::<M>(&view.bar, palette))
        .push(legend.wrap().vertical_spacing(GAP_XS))
        .spacing(GAP_XS);
    if let Some(after) = &view.after {
        content = content.push(line(after, palette.dim_text, false));
    }
    content = content
        .push(theme::horizontal_rule(palette.border_inactive))
        .push(heading_row(&view.columns, palette));
    for row in &view.rows {
        content = content.push(row_of(row, palette));
    }
    if let Some(also) = &view.also {
        content = content.push(line(also, palette.dim_text, false));
    }
    content = content.push(line(&view.footer, palette.dim_text, false));
    let colors = *palette;
    container(content)
        .padding([CARD_PAD_Y, CARD_PAD_X])
        .width(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(colors.background)),
            border: Border {
                color: colors.border_inactive,
                width: 1.0,
                radius: CARD_RADIUS.into(),
            },
            ..container::Style::default()
        })
        .into()
}
