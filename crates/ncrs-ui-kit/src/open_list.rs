//! The list of a dropdown while the keyboard has it open: a few rows around
//! the highlighted one, drawn in the place of the closed field. The caller
//! holds which row is highlighted.

use iced::widget::{button, container, text, Column};
use iced::{Element, Length};

use crate::palette::Palette;
use crate::theme::{self, font_size, spacing};

/// How many rows the open list shows at once.
pub const OPEN_LIST_ROWS: usize = 8;

/// The first row of the window of [`OPEN_LIST_ROWS`] rows that has
/// `highlighted` in it, as near the middle as the ends of the list allow.
pub fn window_start(count: usize, highlighted: usize) -> usize {
    highlighted
        .saturating_sub(OPEN_LIST_ROWS / 2)
        .min(count.saturating_sub(OPEN_LIST_ROWS))
}

/// `labels` as rows that send the `choices` they stand for; the
/// `highlighted` one is filled, the `selected` one (the current value) is
/// marked with a tick.
pub fn open_list<'a, M: Clone + 'a>(
    labels: &[String],
    choices: &[M],
    highlighted: usize,
    selected: usize,
    width: f32,
    palette: &Palette,
) -> Element<'a, M> {
    let start = window_start(labels.len(), highlighted);
    let rows = labels
        .iter()
        .zip(choices)
        .enumerate()
        .skip(start)
        .take(OPEN_LIST_ROWS)
        .map(|(index, (label, choice))| {
            let mark = if index == selected { "✓ " } else { "  " };
            button(
                text(format!("{mark}{label}"))
                    .size(font_size::BODY)
                    .font(crate::APP_FONT)
                    .wrapping(text::Wrapping::None),
            )
            .on_press(choice.clone())
            .padding([2.0, spacing::S])
            .width(Length::Fill)
            .style(theme::menu_item(palette, index == highlighted))
            .into()
        });
    container(Column::with_children(rows))
        .width(width)
        .style(theme::menu(palette))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_follows_the_highlight_and_stops_at_the_ends() {
        assert_eq!(window_start(30, 0), 0);
        assert_eq!(window_start(30, 10), 6);
        assert_eq!(window_start(30, 29), 22);
        assert_eq!(window_start(5, 4), 0);
    }
}
