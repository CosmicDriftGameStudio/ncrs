//! The cards inside a bubble: the areas of a folder, one path with a part
//! marked, and the rows of a trial.
use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{container, text, tooltip, Column, Row, Space};
use iced::{alignment, Background, Border, Color, Element, Length, Pixels, Theme};

use crate::format;
use crate::palette::Palette;
use crate::theme;

use super::*;

const CARD_PAD_Y: f32 = 8.0;
const CARD_PAD_X: f32 = 12.0;
const CARD_RADIUS: f32 = 8.0;
const CARD_LINE: f32 = 20.0;
const SEGMENT_PAD_X: f32 = 4.0;
const SEGMENT_TINT: f32 = 0.22;
const PATH_SEPARATOR: &str = "/";
const ARROW: &str = "→";
const QUESTION_MARK: &str = "?";
const NAME_MAX_WIDTH: f32 = 160.0;
const AREA_NAME_MAX_WIDTH: f32 = 2.0 * NAME_MAX_WIDTH;
/// A path segment longer than this is cut in the middle so a path wraps
/// between segments instead of running out of the card.
const SEGMENT_MAX_CHARS: usize = 28;
const TOOLTIP_PAD: [f32; 2] = [2.0, 6.0];

fn card<'a, M: 'a>(content: Element<'a, M>, palette: &Palette) -> Element<'a, M> {
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

fn card_text<'a, M: 'a>(
    content: &str,
    color: Color,
    wrapping: Wrapping,
    bold: bool,
) -> Element<'a, M> {
    let label = text(content.to_owned())
        .size(TEXT_BODY)
        .line_height(LineHeight::Absolute(Pixels(CARD_LINE)))
        .color(color)
        .wrapping(wrapping);
    if bold {
        label.font(semibold()).into()
    } else {
        label.into()
    }
}

/// A row of the areas card: a name with what it holds and how many documents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AreaLine {
    pub name: String,
    pub detail: String,
    /// A real file path laid out like this area, under the row; empty for none.
    pub example: String,
    pub count: String,
    /// A row for what is not an area of its own, dimmed.
    pub dim: bool,
}

/// The areas of a folder, one row each.
pub fn areas_card<'a, M: 'a>(lines: &[AreaLine], palette: &Palette) -> Element<'a, M> {
    let rows = lines.iter().map(|line| {
        let ink = if line.dim {
            palette.dim_text
        } else {
            palette.text
        };
        let name = card_text(&line.name, ink, Wrapping::WordOrGlyph, !line.dim);
        // The name wraps inside the card, so the count on the right never
        // has to give way. Beside a detail the name keeps a share of the row.
        let title: Element<'a, M> = if line.detail.is_empty() {
            container(name).width(Length::Fill).into()
        } else {
            Row::new()
                .push(container(name).max_width(AREA_NAME_MAX_WIDTH))
                .push(
                    container(card_text(
                        &line.detail,
                        palette.dim_text,
                        Wrapping::Word,
                        false,
                    ))
                    .width(Length::Fill),
                )
                .spacing(GAP_S)
                .align_y(alignment::Vertical::Center)
                .into()
        };
        let head = Row::new()
            .push(container(title).width(Length::Fill))
            .push(card_text(
                &line.count,
                palette.dim_text,
                Wrapping::None,
                false,
            ))
            .spacing(GAP_M)
            .align_y(alignment::Vertical::Center);
        if line.example.is_empty() {
            head.into()
        } else {
            Column::new()
                .push(head)
                .push(card_text(
                    &line.example,
                    palette.dim_text,
                    Wrapping::WordOrGlyph,
                    false,
                ))
                .into()
        }
    });
    card(Column::with_children(rows).spacing(GAP_XS).into(), palette)
}

/// `content` with the full `name` in a small box on hover when `shown` is a
/// shortened form of it.
fn with_full_name<'a, M: 'a>(
    content: Element<'a, M>,
    shown: &str,
    name: &str,
    palette: &Palette,
) -> Element<'a, M> {
    if shown == name {
        return content;
    }
    tooltip(
        content,
        container(
            text(name.to_owned())
                .size(TEXT_BODY)
                .wrapping(Wrapping::None),
        )
        .padding(TOOLTIP_PAD)
        .style(theme::dialog(palette)),
        tooltip::Position::Top,
    )
    .into()
}

fn path_pieces<'a, M: 'a>(
    segments: &[String],
    highlighted: Option<usize>,
    palette: &Palette,
) -> Element<'a, M> {
    let colors = *palette;
    let mut pieces: Vec<Element<'a, M>> = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        if index > 0 {
            pieces.push(mono(PATH_SEPARATOR, TEXT_BODY, colors.dim_text));
        }
        let shown = format::ellipsize_middle(segment, SEGMENT_MAX_CHARS);
        if highlighted == Some(index) {
            let marked = container(mono(shown.clone(), TEXT_BODY, colors.accent))
                .padding([0.0, SEGMENT_PAD_X])
                .style(move |_: &Theme| container::Style {
                    background: Some(Background::Color(colors.accent.scale_alpha(SEGMENT_TINT))),
                    border: Border {
                        color: colors.accent,
                        width: 1.0,
                        radius: theme::radius::CONTROL.into(),
                    },
                    ..container::Style::default()
                })
                .into();
            pieces.push(with_full_name(marked, &shown, segment, palette));
        } else {
            let plain = mono(shown.clone(), TEXT_BODY, colors.text);
            pieces.push(with_full_name(plain, &shown, segment, palette));
        }
    }
    Row::with_children(pieces)
        .spacing(GAP_XS)
        .wrap()
        .align_x(alignment::Horizontal::Left)
        .into()
}

/// One file as a path in segments; the segment at `highlighted` is marked.
pub fn file_card<'a, M: 'a>(
    segments: &[String],
    highlighted: Option<usize>,
    palette: &Palette,
) -> Element<'a, M> {
    card(path_pieces(segments, highlighted, palette), palette)
}

/// A choice below an unsure row.
#[derive(Debug, Clone)]
pub struct Choice<M> {
    pub label: String,
    pub selected: bool,
    pub on_press: M,
}

/// A row of the trial card.
#[derive(Debug, Clone)]
pub enum TrialLine<M> {
    Filed {
        name: String,
        segments: Vec<String>,
    },
    Unsure {
        name: String,
        question: String,
        choices: Vec<Choice<M>>,
    },
    /// A file that is not filed: why.
    Note {
        name: String,
        note: String,
    },
}

fn name_cell<'a, M: 'a>(name: &str, palette: &Palette) -> Element<'a, M> {
    let shown = format::fit_middle(name, NAME_MAX_WIDTH, TEXT_BODY);
    let cell = container(card_text(&shown, palette.text, Wrapping::None, true))
        .max_width(NAME_MAX_WIDTH)
        .clip(true)
        .into();
    with_full_name(cell, &shown, name, palette)
}

fn trial_row<'a, M: Clone + 'a>(line: TrialLine<M>, palette: &Palette) -> Element<'a, M> {
    match line {
        TrialLine::Filed { name, segments } => Row::new()
            .push(name_cell(&name, palette))
            .push(card_text(ARROW, palette.dim_text, Wrapping::None, false))
            .push(path_pieces(&segments, None, palette))
            .spacing(GAP_S)
            .align_y(alignment::Vertical::Center)
            .into(),
        TrialLine::Note { name, note } => Row::new()
            .push(name_cell(&name, palette))
            .push(card_text(ARROW, palette.dim_text, Wrapping::None, false))
            .push(card_text(&note, palette.dim_text, Wrapping::Word, false))
            .spacing(GAP_S)
            .align_y(alignment::Vertical::Center)
            .into(),
        TrialLine::Unsure {
            name,
            question,
            choices,
        } => {
            let chips = choices.into_iter().map(|choice| {
                chat_chip_small(&choice.label, choice.selected, choice.on_press, palette)
            });
            Column::new()
                .push(
                    Row::new()
                        .push(name_cell(&name, palette))
                        .push(card_text(
                            QUESTION_MARK,
                            palette.accent,
                            Wrapping::None,
                            true,
                        ))
                        .push(card_text(&question, palette.text, Wrapping::Word, false))
                        .spacing(GAP_S)
                        .align_y(alignment::Vertical::Center),
                )
                .push(chip_row(chips.collect()))
                .spacing(GAP_XS)
                .into()
        }
    }
}

/// What a trial would do with the files of a page, row by row. `more` closes
/// the card when there are more files than the page shows.
pub fn trial_card<'a, M: Clone + 'a>(
    header: &str,
    lines: Vec<TrialLine<M>>,
    more: Option<&str>,
    palette: &Palette,
) -> Element<'a, M> {
    let mut rows = Column::new()
        .push(card_text(header, palette.dim_text, Wrapping::Word, true))
        .push(Space::new().height(GAP_XS))
        .spacing(GAP_XS);
    for line in lines {
        rows = rows.push(trial_row(line, palette));
    }
    if let Some(rest) = more {
        rows = rows.push(card_text(rest, palette.dim_text, Wrapping::Word, false));
    }
    card(rows.into(), palette)
}
