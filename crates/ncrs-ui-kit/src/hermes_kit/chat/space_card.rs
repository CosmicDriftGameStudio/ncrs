//! The card of the Platz answer: where the space of the drive is, as a bar,
//! the legend and a table of the biggest folders. The buttons that act on it
//! are chips of the chat, not part of the card.
use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{container, rich_text, text, Column, Row};
use iced::{alignment, Background, Border, Color, Element, Length, Pixels, Theme};

use crate::color_chip::color_chip;
use crate::palette::Palette;
use crate::stacked_bar::{stacked_bar, BarSegment};
use crate::theme;

use super::bubble::spans_of;
use super::*;

const CARD_PAD_Y: f32 = 8.0;
const CARD_PAD_X: f32 = 12.0;
const CARD_RADIUS: f32 = 8.0;
const LINE: f32 = 20.0;
const SIZE_COLUMN: f32 = 80.0;
const GROUP_COLUMN: f32 = 130.0;
const NAME_PORTION: u16 = 3;
const HINT_PORTION: u16 = 4;
const ROW_BAR_WIDTH: f32 = 48.0;
const SELECTED_ALPHA: f32 = 0.3;
const SWATCH: f32 = 8.0;

/// One folder of the table.
#[derive(Debug, Clone, PartialEq)]
pub struct SpaceCardRow {
    pub name: String,
    /// Where it lies, dim after the name; empty for none.
    pub path: String,
    pub size: String,
    /// The parts of the size by group, drawn as a small bar scaled to the biggest row.
    pub parts: Vec<BarSegment>,
    /// One chip per group the row has.
    pub groups: Vec<(String, Color)>,
    pub hint: String,
    /// The row the cursor stands on.
    pub selected: bool,
}

/// One entry of the legend: a swatch, the label and the figure in dim text.
#[derive(Debug, Clone, PartialEq)]
pub struct SpaceLegendItem {
    pub label: String,
    pub size: String,
    pub color: Color,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpaceCardView {
    pub title: String,
    /// The drive and its size, right after the title.
    pub drive: String,
    pub bar: Vec<BarSegment>,
    /// The legend under the bar: a label with its figure and the color of its part.
    pub legend: Vec<SpaceLegendItem>,
    /// What the drive would look like after the safe ones went, right-aligned in the head; `**bold**` is set in bold.
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

fn row_of<'a, M: Clone + 'a>(row: &SpaceCardRow, widest: u64, palette: &Palette) -> Element<'a, M> {
    let mut name = Row::new()
        .push(line(&row.name, palette.text, true))
        .spacing(GAP_S);
    if !row.path.is_empty() {
        name = name.push(line(&row.path, palette.dim_text, false));
    }
    let total: u64 = row.parts.iter().map(|part| part.portion).sum();
    let bar_width = if widest == 0 {
        0.0
    } else {
        ROW_BAR_WIDTH * (total as f32 / widest as f32)
    };
    let mut size = Row::new()
        .push(line(&row.size, palette.text, false))
        .spacing(GAP_S)
        .align_y(alignment::Vertical::Center);
    if bar_width >= 1.0 {
        size = size
            .push(container(stacked_bar::<M>(&row.parts, palette)).width(Length::Fixed(bar_width)));
    }
    let mut chips = Row::new().spacing(GAP_XS);
    for (label, color) in &row.groups {
        chips = chips.push(color_chip::<M>(label, *color, None));
    }
    let colors = *palette;
    let selected = row.selected;
    container(
        Row::new()
            .push(container(name).width(Length::FillPortion(NAME_PORTION)))
            .push(
                container(size)
                    .width(SIZE_COLUMN)
                    .align_x(alignment::Horizontal::Right),
            )
            .push(
                container(chips.wrap().vertical_spacing(GAP_XS))
                    .width(GROUP_COLUMN)
                    .padding([0.0, GAP_S]),
            )
            .push(
                container(line(&row.hint, palette.dim_text, false))
                    .width(Length::FillPortion(HINT_PORTION)),
            )
            .spacing(GAP_S)
            .align_y(alignment::Vertical::Center),
    )
    .style(move |_: &Theme| container::Style {
        background: selected.then(|| Background::Color(colors.cursor.scale_alpha(SELECTED_ALPHA))),
        ..container::Style::default()
    })
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

fn legend_item<'a, M: 'a>(item: &SpaceLegendItem, palette: &Palette) -> Element<'a, M> {
    let color = item.color;
    let edge = palette.border_inactive;
    Row::new()
        .push(
            container("")
                .width(SWATCH)
                .height(SWATCH)
                .style(move |_: &Theme| container::Style {
                    background: Some(Background::Color(color)),
                    border: Border {
                        color: edge,
                        width: 1.0,
                        radius: 2.0.into(),
                    },
                    ..container::Style::default()
                }),
        )
        .push(line(&item.label, palette.text, false))
        .push(line(&item.size, palette.dim_text, false))
        .spacing(GAP_XS)
        .align_y(alignment::Vertical::Center)
        .into()
}

/// A line whose `**bold**` stretches are set in bold.
fn marked_line<'a, M: 'a>(content: &str, color: Color) -> Element<'a, M> {
    rich_text(spans_of(content, color))
        .size(TEXT_SMALL)
        .line_height(LineHeight::Absolute(Pixels(LINE)))
        .wrapping(Wrapping::None)
        .into()
}

/// `actions` (the chips that act on the card) sit inside the card, above its footer.
pub fn space_card<'a, M: Clone + 'a>(
    view: &SpaceCardView,
    actions: Option<Element<'a, M>>,
    palette: &Palette,
) -> Element<'a, M> {
    let mut head = Row::new()
        .push(line(&view.title, palette.text, true))
        .push(line(&view.drive, palette.dim_text, false))
        .spacing(GAP_S);
    if let Some(after) = &view.after {
        head = head.push(
            container(marked_line(after, palette.dim_text))
                .width(Length::Fill)
                .align_x(alignment::Horizontal::Right),
        );
    }
    let mut legend = Row::new().spacing(GAP_S);
    for item in &view.legend {
        legend = legend.push(legend_item::<M>(item, palette));
    }
    let mut content = Column::new()
        .push(head)
        .push(stacked_bar::<M>(&view.bar, palette))
        .push(legend.wrap().vertical_spacing(GAP_XS))
        .spacing(GAP_XS);
    content = content
        .push(theme::horizontal_rule(palette.border_inactive))
        .push(heading_row(&view.columns, palette));
    let widest = view
        .rows
        .iter()
        .map(|row| row.parts.iter().map(|part| part.portion).sum::<u64>())
        .max()
        .unwrap_or(0);
    for row in &view.rows {
        content = content.push(row_of(row, widest, palette));
    }
    if let Some(also) = &view.also {
        content = content.push(line(also, palette.dim_text, false));
    }
    if let Some(action_row) = actions {
        content = content.push(action_row);
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
