//! A row of a list by size: tick, a bar against the biggest row, name and
//! folder, the reason, the size. Used by the Platz tab for every group.

use iced::widget::text::Wrapping;
use iced::widget::{column, container, mouse_area, row, text, Space};
use iced::{alignment, Color, Element, Length};

use crate::format::ellipsize_middle;
use crate::git_rows::{ref_chip, RefChipKind};
use crate::git_state::RunOutcome;
use crate::kit::{self, icon, Check};
use crate::layout::SIZE_COLUMN_WIDTH;
use crate::palette::Palette;
use crate::theme::git;
use crate::theme::{self, font_size, size, spacing};

/// What fits under the name before the folder is cut in the middle.
const PATH_CHARS: usize = 60;
const TICK_CELL: f32 = 28.0;
const BAR_PORTIONS: u16 = 1000;
const BAR_HEIGHT: f32 = 8.0;
/// How many stripes a full bar of the gone look has.
const HATCH_STRIPES: u16 = 12;

pub struct SizeRow<M> {
    /// `None` for a row that cannot be ticked.
    pub check: Option<Check>,
    pub name: String,
    pub path: String,
    pub size_text: String,
    /// How big the row is against the biggest of the list, `0.0` to `1.0`.
    pub fraction: f32,
    pub color: Color,
    pub reason: String,
    /// What became of the row in a run; takes the place of the reason.
    pub outcome: Option<(RunOutcome, String)>,
    pub cursor: bool,
    pub on_press: Option<M>,
    pub on_toggle: Option<M>,
}

impl<M> std::fmt::Debug for SizeRow<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SizeRow")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

fn filled_portions(fraction: f32) -> u16 {
    if fraction <= 0.0 {
        return 0;
    }
    let portions = (fraction.clamp(0.0, 1.0) * f32::from(BAR_PORTIONS)).round() as u16;
    portions.max(BAR_PORTIONS.div_euclid(60))
}

/// The bar of a row against the biggest of the list.
pub fn size_bar<'a, M: 'a>(fraction: f32, color: Color, palette: &Palette) -> Element<'a, M> {
    bar_of(fraction, color, false, palette)
}

/// A bar for what is gone: the filled part is cut into stripes.
pub fn size_bar_hatched<'a, M: 'a>(
    fraction: f32,
    color: Color,
    palette: &Palette,
) -> Element<'a, M> {
    bar_of(fraction, color, true, palette)
}

fn bar_of<'a, M: 'a>(
    fraction: f32,
    color: Color,
    hatched: bool,
    palette: &Palette,
) -> Element<'a, M> {
    let filled = filled_portions(fraction);
    let part = |portion: u16, tint: Color| {
        container("")
            .width(Length::FillPortion(portion))
            .height(BAR_HEIGHT)
            .style(theme::line(tint))
    };
    let mut bar = row![];
    if filled > 0 {
        if hatched {
            let stripe = filled.div_euclid(2 * HATCH_STRIPES).max(1);
            let mut left = filled;
            while left > 0 {
                let width = stripe.min(left);
                bar = bar.push(part(width, color));
                left -= width;
                if left > 0 {
                    let gap = stripe.min(left);
                    bar = bar.push(part(gap, color.scale_alpha(0.25)));
                    left -= gap;
                }
            }
        } else {
            bar = bar.push(part(filled, color));
        }
    }
    if filled < BAR_PORTIONS {
        bar = bar.push(part(
            BAR_PORTIONS - filled,
            palette.border_inactive.scale_alpha(0.4),
        ));
    }
    container(bar).width(size::SIZE_BAR_WIDTH).into()
}

fn wrapped_line<'a, M: 'a>(content: String, font: f32, ink: Color) -> Element<'a, M> {
    text(content)
        .size(font)
        .color(ink)
        .wrapping(Wrapping::Word)
        .into()
}

fn line<'a, M: 'a>(content: String, font: f32, ink: Color) -> Element<'a, M> {
    text(content)
        .size(font)
        .color(ink)
        .wrapping(Wrapping::None)
        .into()
}

/// How the check of a row reads: done and fine, something the user decides, or a stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckTone {
    Ok,
    Decide,
    Blocked,
}

impl CheckTone {
    fn ink(self, palette: &Palette) -> Color {
        match self {
            Self::Ok => palette.success,
            Self::Decide => theme::space::decide(),
            Self::Blocked => palette.error,
        }
    }
}

/// What a row can have beyond [`SizeRow`]: the check column, a branch chip, the look of
/// what is gone, and a button at the end.
pub struct SizeRowExt<M> {
    /// The row has its own check cell, empty without `check`.
    pub check_column: bool,
    pub check: Option<(CheckTone, String)>,
    /// A ref chip after the name, like the branch of a worktree.
    pub chip: Option<String>,
    /// The thing is gone: dim name, striped bar.
    pub gone: bool,
    /// A button in a cell at the end of the row.
    pub action: Option<(String, M)>,
    /// The row has the cell for the button, empty without `action`.
    pub action_column: bool,
}

impl<M> Default for SizeRowExt<M> {
    fn default() -> Self {
        Self {
            check_column: false,
            check: None,
            chip: None,
            gone: false,
            action: None,
            action_column: false,
        }
    }
}

pub fn size_row<'a, M: Clone + 'a>(item: SizeRow<M>, palette: &Palette) -> Element<'a, M> {
    size_row_ext(item, SizeRowExt::default(), palette)
}

pub fn size_row_ext<'a, M: Clone + 'a>(
    item: SizeRow<M>,
    ext: SizeRowExt<M>,
    palette: &Palette,
) -> Element<'a, M> {
    let (ink, dim_ink) = if item.cursor {
        (palette.cursor_text, palette.cursor_text)
    } else if ext.gone {
        (palette.dim_text, palette.dim_text)
    } else {
        (palette.text, palette.dim_text)
    };
    let tick: Element<'a, M> = match item.check {
        Some(state) => kit::checkbox(state, item.on_toggle, palette),
        None => Space::new().into(),
    };
    let name_line: Element<'a, M> = match ext.chip {
        Some(branch) => row![
            line(item.name, font_size::BODY, ink),
            ref_chip(branch, RefChipKind::Branch, palette)
        ]
        .spacing(spacing::XS)
        .align_y(alignment::Vertical::Center)
        .into(),
        None => line(item.name, font_size::BODY, ink),
    };
    let names = column![
        name_line,
        line(
            ellipsize_middle(&item.path, PATH_CHARS),
            font_size::SMALL,
            dim_ink
        ),
    ];
    let reason: Element<'a, M> = match item.outcome {
        Some((outcome, words)) => {
            let outcome_ink = outcome.color(palette);
            row![
                icon(outcome.icon(), outcome_ink),
                wrapped_line(words, font_size::SMALL, outcome_ink)
            ]
            .spacing(spacing::XS)
            .align_y(alignment::Vertical::Center)
            .into()
        }
        None => wrapped_line(item.reason, font_size::SMALL, dim_ink),
    };
    let bar = if ext.gone {
        size_bar_hatched(item.fraction, item.color, palette)
    } else {
        size_bar(item.fraction, item.color, palette)
    };
    let mut cells = row![
        container(tick).width(TICK_CELL),
        bar,
        container(names)
            .width(Length::FillPortion(size::SIZE_NAME_PORTION))
            .clip(true),
        container(reason)
            .width(Length::FillPortion(size::SIZE_REASON_PORTION))
            .clip(true),
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    if ext.check_column {
        let cell: Element<'a, M> = match ext.check {
            Some((tone, words)) => {
                let tint = if item.cursor {
                    palette.cursor_text
                } else {
                    tone.ink(palette)
                };
                let shown = match tone {
                    CheckTone::Ok => format!("\u{2713} {words}"),
                    CheckTone::Decide | CheckTone::Blocked => words,
                };
                wrapped_line(shown, font_size::SMALL, tint)
            }
            None => Space::new().into(),
        };
        cells = cells.push(
            container(cell)
                .width(Length::FillPortion(size::SIZE_CHECK_PORTION))
                .clip(true),
        );
    }
    cells = cells.push(
        container(line(item.size_text, font_size::BODY, ink))
            .width(SIZE_COLUMN_WIDTH)
            .align_x(alignment::Horizontal::Right),
    );
    if ext.action_column {
        let cell: Element<'a, M> = match ext.action {
            Some((label, message)) => {
                kit::button(&label, Some(message), kit::Tone::Secondary, palette)
            }
            None => Space::new().into(),
        };
        cells = cells.push(
            container(cell)
                .width(size::SIZE_ACTION_WIDTH)
                .align_x(alignment::Horizontal::Right),
        );
    }
    let framed = container(cells)
        .padding([0.0, spacing::XL])
        .width(Length::Fill)
        // Fixed: the list's scroll and menu maths count rows of exactly this height; two wrapped lines fit.
        .height(size::SIZE_ROW)
        .clip(true)
        .align_y(alignment::Vertical::Center)
        .style(git::marked_row(palette, item.cursor, false));
    match item.on_press {
        Some(message) => mouse_area(framed).on_press(message).into(),
        None => framed.into(),
    }
}

/// The words over the columns of [`size_row_ext`], in the same widths.
#[derive(Debug, Clone)]
pub struct SizeRowHeader {
    pub bar: String,
    pub name: String,
    pub reason: String,
    pub check: Option<String>,
    pub size: String,
    pub action_column: bool,
}

pub fn size_row_header<'a, M: 'a>(header: SizeRowHeader, palette: &Palette) -> Element<'a, M> {
    let label = |words: String| -> Element<'a, M> {
        text(words.to_uppercase())
            .size(font_size::SMALL)
            .color(palette.dim_text)
            .wrapping(Wrapping::None)
            .into()
    };
    let mut cells = row![
        Space::new().width(TICK_CELL),
        container(label(header.bar)).width(size::SIZE_BAR_WIDTH),
        container(label(header.name)).width(Length::FillPortion(size::SIZE_NAME_PORTION)),
        container(label(header.reason)).width(Length::FillPortion(size::SIZE_REASON_PORTION)),
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    if let Some(check) = header.check {
        cells = cells
            .push(container(label(check)).width(Length::FillPortion(size::SIZE_CHECK_PORTION)));
    }
    cells = cells.push(
        container(label(header.size))
            .width(SIZE_COLUMN_WIDTH)
            .align_x(alignment::Horizontal::Right),
    );
    if header.action_column {
        cells = cells.push(Space::new().width(size::SIZE_ACTION_WIDTH));
    }
    column![
        container(cells)
            .padding([0.0, spacing::XL])
            .width(Length::Fill)
            .height(size::COLUMN_HEADER)
            .align_y(alignment::Vertical::Center),
        theme::horizontal_rule(palette.border_inactive),
    ]
    .into()
}

impl<M> std::fmt::Debug for SizeRowExt<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SizeRowExt")
            .field("gone", &self.gone)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_small_row_keeps_a_visible_bar_and_an_empty_one_has_none() {
        assert_eq!(filled_portions(0.0), 0);
        assert!(filled_portions(0.001) >= BAR_PORTIONS.div_euclid(60));
        assert_eq!(filled_portions(1.0), BAR_PORTIONS);
        assert_eq!(filled_portions(5.0), BAR_PORTIONS);
    }
}
