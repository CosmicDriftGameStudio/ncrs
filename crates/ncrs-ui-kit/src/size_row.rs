//! A row of a list by size: tick, a bar against the biggest row, name and
//! folder, the reason, the size. Used by the Platz tab for every group.

use iced::widget::text::Wrapping;
use iced::widget::{column, container, mouse_area, row, text, Space};
use iced::{alignment, Color, Element, Length};

use crate::format::ellipsize_middle;
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
    portions.max(BAR_PORTIONS / 60)
}

fn size_bar<'a, M: 'a>(fraction: f32, color: Color, palette: &Palette) -> Element<'a, M> {
    let filled = filled_portions(fraction);
    let part = |portion: u16, tint: Color| {
        container("")
            .width(Length::FillPortion(portion))
            .height(BAR_HEIGHT)
            .style(theme::line(tint))
    };
    let mut bar = row![];
    if filled > 0 {
        bar = bar.push(part(filled, color));
    }
    if filled < BAR_PORTIONS {
        bar = bar.push(part(
            BAR_PORTIONS - filled,
            palette.border_inactive.scale_alpha(0.4),
        ));
    }
    container(bar).width(size::SIZE_BAR_WIDTH).into()
}

fn line<'a, M: 'a>(content: String, font: f32, ink: Color) -> Element<'a, M> {
    text(content)
        .size(font)
        .color(ink)
        .wrapping(Wrapping::None)
        .into()
}

pub fn size_row<'a, M: Clone + 'a>(item: SizeRow<M>, palette: &Palette) -> Element<'a, M> {
    let (ink, dim_ink) = if item.cursor {
        (palette.cursor_text, palette.cursor_text)
    } else {
        (palette.text, palette.dim_text)
    };
    let tick: Element<'a, M> = match item.check {
        Some(state) => kit::checkbox(state, item.on_toggle, palette),
        None => Space::new().into(),
    };
    let names = column![
        line(item.name, font_size::BODY, ink),
        line(
            ellipsize_middle(&item.path, PATH_CHARS),
            font_size::SMALL,
            dim_ink
        ),
    ];
    let reason: Element<'a, M> = match item.outcome {
        Some((outcome, words)) => {
            let ink = outcome.color(palette);
            row![
                icon(outcome.icon(), ink),
                line(words, font_size::SMALL, ink)
            ]
            .spacing(spacing::XS)
            .align_y(alignment::Vertical::Center)
            .into()
        }
        None => line(item.reason, font_size::SMALL, dim_ink),
    };
    let cells = row![
        container(tick).width(TICK_CELL),
        size_bar(item.fraction, item.color, palette),
        container(names).width(Length::Fill).clip(true),
        container(reason).width(size::SIZE_REASON_WIDTH).clip(true),
        container(line(item.size_text, font_size::BODY, ink))
            .width(SIZE_COLUMN_WIDTH)
            .align_x(alignment::Horizontal::Right),
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    let framed = container(cells)
        .padding([0.0, spacing::XL])
        .width(Length::Fill)
        .height(size::SIZE_ROW)
        .align_y(alignment::Vertical::Center)
        .style(git::marked_row(palette, item.cursor, false));
    match item.on_press {
        Some(message) => mouse_area(framed).on_press(message).into(),
        None => framed.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_small_row_keeps_a_visible_bar_and_an_empty_one_has_none() {
        assert_eq!(filled_portions(0.0), 0);
        assert!(filled_portions(0.001) >= BAR_PORTIONS / 60);
        assert_eq!(filled_portions(1.0), BAR_PORTIONS);
        assert_eq!(filled_portions(5.0), BAR_PORTIONS);
    }
}
