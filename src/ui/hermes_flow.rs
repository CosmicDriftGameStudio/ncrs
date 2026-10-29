//! The picture of how Hermes works: documents travel from the inbox through
//! reading and rules to the filing location. Built from boxes and lines with
//! dots on them; the caller counts `step` up to make the dots move.

use std::borrow::Cow;

use iced::widget::{column, container, text, Row, Space, Stack};
use iced::{alignment, Border, Color, Element, Length, Theme};

use super::theme::{self, font_size, spacing};
use crate::messages::Message;
use crate::palette::Palette;

/// How often the caller advances `step` while the picture is on screen.
pub const STEP_INTERVAL: std::time::Duration = std::time::Duration::from_millis(40);

/// Steps for a dot to cross a line.
const STEPS_PER_CROSSING: u32 = 80;
/// The room a line is divided into: a dot's place is a share of it.
const LINE_PORTIONS: u32 = 1000;
const DOT_SIZE: f32 = 8.0;
const LINE_THICKNESS: f32 = 2.0;
const DASH_COUNT: usize = 24;
const STEP_BOX_WIDTH: f32 = 150.0;
const BOX_GAP: f32 = 14.0;
const LINE_GAP: f32 = 34.0;
const LINE_MARGIN: f32 = 8.0;

/// A box of the picture: a name and, under it, a short line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowBox<'a> {
    pub title: Cow<'a, str>,
    pub note: Option<Cow<'a, str>>,
}

/// The texts of the five boxes: where documents arrive, where the filed ones
/// already are, and the three stages in between.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowBoxes<'a> {
    pub inbox: FlowBox<'a>,
    pub archive: FlowBox<'a>,
    pub reading: FlowBox<'a>,
    pub rules: FlowBox<'a>,
    pub filed: FlowBox<'a>,
}

/// Where along the line, in thousandths, a dot is at `step`; `offset` shifts
/// it so several dots on one line do not travel together.
pub fn dot_position(step: u32, offset: u32) -> u32 {
    let in_crossing = step.wrapping_add(offset) % STEPS_PER_CROSSING;
    (in_crossing * LINE_PORTIONS).div_euclid(STEPS_PER_CROSSING)
}

/// Which dots travel on a line: their color and how far they trail the first.
struct Dots {
    color: Color,
    offsets: &'static [u32],
}

const DOT_SPACING: u32 = STEPS_PER_CROSSING.div_euclid(3);
const THREE_DOTS: &[u32] = &[0, DOT_SPACING, 2 * DOT_SPACING];
const TWO_DOTS: &[u32] = &[0, DOT_SPACING];
const ONE_DOT_LATE: &[u32] = &[DOT_SPACING];
const ONE_DOT_LATER: &[u32] = &[2 * DOT_SPACING];

/// The picture at `step`; step 0 is the resting picture a snapshot shows.
pub fn flow<'a>(step: u32, boxes: FlowBoxes<'a>, palette: &Palette) -> Element<'a, Message> {
    let ends = column![
        step_box(boxes.inbox, palette.accent, false, palette),
        step_box(boxes.archive, palette.border_inactive, true, palette),
    ]
    .spacing(BOX_GAP)
    .width(STEP_BOX_WIDTH);

    let inbound = column![
        solid_line(
            step,
            Dots {
                color: palette.accent,
                offsets: THREE_DOTS,
            },
            palette,
        ),
        dashed_line(
            step,
            Dots {
                color: palette.success,
                offsets: ONE_DOT_LATE,
            },
            palette,
        ),
    ]
    .spacing(LINE_GAP)
    .width(Length::FillPortion(1));

    Row::new()
        .push(ends)
        .push(
            container(inbound)
                .padding([0.0, LINE_MARGIN])
                .width(Length::FillPortion(1)),
        )
        .push(step_box(
            boxes.reading,
            palette.border_inactive,
            false,
            palette,
        ))
        .push(between_boxes(
            step,
            Dots {
                color: palette.accent,
                offsets: TWO_DOTS,
            },
            palette,
        ))
        .push(step_box(
            boxes.rules,
            palette.border_inactive,
            false,
            palette,
        ))
        .push(between_boxes(
            step,
            Dots {
                color: palette.success,
                offsets: ONE_DOT_LATER,
            },
            palette,
        ))
        .push(step_box(boxes.filed, palette.success, false, palette))
        .align_y(alignment::Vertical::Center)
        .width(Length::Fill)
        .into()
}

fn between_boxes<'a>(step: u32, dots: Dots, palette: &Palette) -> Element<'a, Message> {
    container(solid_line(step, dots, palette))
        .padding([0.0, LINE_MARGIN])
        .width(Length::FillPortion(1))
        .into()
}

fn step_box<'a>(
    content: FlowBox<'a>,
    outline: Color,
    quiet: bool,
    palette: &Palette,
) -> Element<'a, Message> {
    let title_color = if quiet {
        palette.dim_text
    } else {
        palette.text
    };
    let mut lines = column![text(content.title).size(font_size::ROW).color(title_color)]
        .spacing(spacing::XS)
        .align_x(alignment::Horizontal::Center);
    if let Some(note) = content.note {
        lines = lines.push(text(note).size(font_size::BODY).color(palette.dim_text));
    }
    container(lines)
        .padding(spacing::L)
        .width(STEP_BOX_WIDTH)
        .align_x(alignment::Horizontal::Center)
        .style(theme::outlined_in(outline))
        .into()
}

fn solid_line<'a>(step: u32, dots: Dots, palette: &Palette) -> Element<'a, Message> {
    let rail = container(Space::new())
        .width(Length::Fill)
        .height(LINE_THICKNESS)
        .style(theme::line(palette.border_inactive));
    with_dots(rail.into(), step, dots)
}

fn dashed_line<'a>(step: u32, dots: Dots, palette: &Palette) -> Element<'a, Message> {
    let mut rail = Row::new().width(Length::Fill).height(LINE_THICKNESS);
    for _ in 0..DASH_COUNT {
        rail = rail
            .push(
                container(Space::new())
                    .width(Length::FillPortion(1))
                    .height(LINE_THICKNESS)
                    .style(theme::line(palette.border_inactive)),
            )
            .push(Space::new().width(Length::FillPortion(1)));
    }
    with_dots(rail.into(), step, dots)
}

fn with_dots<'a>(rail: Element<'a, Message>, step: u32, dots: Dots) -> Element<'a, Message> {
    let mut layers: Vec<Element<'a, Message>> = vec![container(rail)
        .width(Length::Fill)
        .height(DOT_SIZE)
        .align_y(alignment::Vertical::Center)
        .into()];
    for offset in dots.offsets {
        let position = dot_position(step, *offset).clamp(1, LINE_PORTIONS - 1);
        layers.push(
            Row::new()
                .push(Space::new().width(Length::FillPortion(u16::try_from(position).unwrap_or(1))))
                .push(dot(dots.color))
                .push(Space::new().width(Length::FillPortion(
                    u16::try_from(LINE_PORTIONS - position).unwrap_or(1),
                )))
                .width(Length::Fill)
                .height(DOT_SIZE)
                .into(),
        );
    }
    Stack::with_children(layers)
        .width(Length::Fill)
        .height(DOT_SIZE)
        .into()
}

fn dot<'a>(color: Color) -> Element<'a, Message> {
    container(Space::new())
        .width(DOT_SIZE)
        .height(DOT_SIZE)
        .style(move |_theme: &Theme| container::Style {
            background: Some(color.into()),
            border: Border {
                radius: (DOT_SIZE / 2.0).into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

// reason: a failing assertion is the signal in a test
#[allow(clippy::indexing_slicing)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dot_starts_at_the_left_and_moves_right_until_the_crossing_is_done() {
        assert_eq!(dot_position(0, 0), 0);
        assert!(dot_position(10, 0) > dot_position(5, 0));
        assert!(dot_position(STEPS_PER_CROSSING - 1, 0) < LINE_PORTIONS);
        assert_eq!(dot_position(STEPS_PER_CROSSING, 0), 0);
    }

    #[test]
    fn dots_with_different_offsets_are_in_different_places() {
        let places: Vec<u32> = THREE_DOTS
            .iter()
            .map(|offset| dot_position(0, *offset))
            .collect();
        assert!(
            places
                .windows(2)
                .all(|pair| matches!(pair, [first, second] if first != second)),
            "{places:?}"
        );
    }

    #[test]
    fn the_counter_may_wrap_around() {
        assert!(dot_position(u32::MAX, DOT_SPACING) < LINE_PORTIONS);
    }
}
