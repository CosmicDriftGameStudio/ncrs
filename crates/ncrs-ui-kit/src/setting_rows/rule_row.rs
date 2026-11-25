//! A rule of a target as a row: its texts in columns.

use iced::widget::text::Wrapping;
use iced::widget::{container, responsive, row, text};
use iced::{alignment, Color, Element, Length};
use crate::palette::Palette;

use crate::format::fit_end;
use crate::theme::{font_size, spacing};

use super::{button_row, ink, pressable, status_text, RowButton, RowLayout, SettingRow};

/// The columns of a rule; the condition and the pattern share what is left.
const RULE_NAME_WIDTH: f32 = 190.0;
const RULE_CONDITION_PORTION: u16 = 3;
const RULE_PATTERN_PORTION: u16 = 4;
const RULE_HITS_WIDTH: f32 = 104.0;
/// One line of text in the body size.
const CELL_HEIGHT: f32 = 16.0;

/// The texts of a rule, one per column.
#[derive(Debug, Clone)]
pub struct RuleCells {
    pub name: String,
    pub condition: String,
    pub pattern: String,
    pub hits: String,
}

impl<'a, M: Clone + 'a> SettingRow<'a, M> {
    /// A rule of a target on one line: the tick, the texts in columns that
    /// stand at the same place in every rule, and the buttons. A text that is
    /// too long for its column is cut.
    pub fn listed_rule(
        tick: RowButton<M>,
        cells: &RuleCells,
        buttons: Vec<RowButton<M>>,
        palette: &Palette,
    ) -> Self {
        let before_buttons = usize::from(tick.reachable());
        let mut presses = pressable(std::slice::from_ref(&tick));
        presses.extend(pressable(&buttons));
        let build = |marked: Option<usize>| {
            let colors = ink(palette, marked.is_some());
            let marked_button = marked.and_then(|at| at.checked_sub(before_buttons));
            row![
                button_row(None, std::slice::from_ref(&tick), marked, palette),
                cell(
                    cells.name.clone(),
                    Length::Fixed(RULE_NAME_WIDTH),
                    colors.text,
                ),
                cell(
                    cells.condition.clone(),
                    Length::FillPortion(RULE_CONDITION_PORTION),
                    colors.dim_text,
                ),
                cell(
                    cells.pattern.clone(),
                    Length::FillPortion(RULE_PATTERN_PORTION),
                    colors.text,
                ),
                container(status_text(cells.hits.clone(), colors.dim_text))
                    .width(RULE_HITS_WIDTH)
                    .align_x(alignment::Horizontal::Right),
                button_row(None, &buttons, marked_button, palette),
            ]
            .spacing(spacing::L)
            .align_y(alignment::Vertical::Center)
            .into()
        };
        Self {
            layout: RowLayout::Bare,
            list_indent: Some(spacing::XXL),
            ..Self::text(
                if cells.name.is_empty() {
                    cells.condition.clone()
                } else {
                    cells.name.clone()
                },
                Some(cells.condition.clone()),
                build(None),
            )
        }
        .with_buttons(presses, build)
    }
}

/// A text on one line, cut to the width the cell gets.
fn cell<'a, M: Clone + 'a>(content: String, width: Length, color: Color) -> Element<'a, M> {
    container(responsive(move |room| {
        text(fit_end(&content, room.width, font_size::BODY))
            .size(font_size::BODY)
            .font(crate::APP_FONT)
            .color(color)
            .wrapping(Wrapping::None)
            .into()
    }))
    .width(width)
    .height(CELL_HEIGHT)
    .into()
}
