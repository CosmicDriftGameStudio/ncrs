//! The count summary of a selection.

use iced::widget::button as widget_button;
use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, text, Row};
use iced::{alignment, Element, Length};

use crate::palette::Palette;
use crate::theme::{self, font_size, size, spacing};

use super::*;

/// One number of a [`count_summary`], or the dimmed stand-in for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Figure {
    Value(String),
    /// Shown quietly in place of a number: not counted yet, or not countable.
    Note(String),
}

fn figure_cells<'a, M: Clone + 'a>(figures: &[(&str, Figure)], palette: &Palette) -> Row<'a, M> {
    let cells = figures.iter().map(|(label, figure)| {
        let shown: Element<'a, M> = match figure {
            Figure::Value(value) => text(value.clone())
                .size(font_size::FIGURE)
                .color(palette.text)
                .wrapping(Wrapping::None)
                .into(),
            Figure::Note(note) => dim(note.clone(), palette),
        };
        container(column![dim(label.to_string(), palette), shown].spacing(spacing::XS / 2.0))
            .padding([spacing::M, BUTTON_PADDING_X])
            .width(Length::FillPortion(1))
            .into()
    });
    Row::with_children(cells)
}

/// The numbers of [`count_summary`] alone, in the same outlined box and without
/// the button.
pub fn figure_row<'a, M: Clone + 'a>(
    figures: &[(&str, Figure)],
    palette: &Palette,
) -> Element<'a, M> {
    container(figure_cells(figures, palette))
        .style(theme::outlined(palette))
        .into()
}

/// The size of a selection in a few numbers, with a button that shows or hides
/// the list behind them; `note` goes between the two, set off by a divider.
pub fn count_summary<'a, M: Clone + 'a>(
    figures: &[(&str, Figure)],
    note: Option<Element<'a, M>>,
    expanded: bool,
    on_toggle: M,
    show_label: &str,
    hide_label: &str,
    palette: &Palette,
) -> Element<'a, M> {
    let (chevron, label) = if expanded {
        ("v", hide_label)
    } else {
        (">", show_label)
    };
    let toggle = widget_button(
        container(
            row![label_text(chevron), label_text(label)]
                .spacing(spacing::M)
                .align_y(alignment::Vertical::Center),
        )
        .center_y(Length::Fill),
    )
    .padding([0.0, BUTTON_PADDING_X])
    .height(size::ROW_TALL)
    .width(Length::Fill)
    .on_press(on_toggle)
    .style(theme::link_button(palette));
    let mut parts = column![figure_cells(figures, palette), divider(palette)];
    if let Some(between) = note {
        parts = parts.push(container(between).padding([spacing::M, BUTTON_PADDING_X]));
        parts = parts.push(divider(palette));
    }
    container(parts.push(toggle))
        .style(theme::outlined(palette))
        .into()
}
