//! A bar for every day, each split by what made it up (a model), with a legend
//! of what the colors mean. Built from `FillPortion` blocks like the bar of
//! [`crate::meter`], so it needs no canvas.

use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, text, Column, Row};
use iced::{alignment, Color, Element, Length};

use crate::palette::Palette;
use crate::theme::{self, font_size, spacing};

const BARS_HEIGHT: f32 = 160.0;
const BAR_GAP: f32 = 2.0;
const SWATCH_SIDE: f32 = 8.0;
/// How finely a bar is divided; a piece smaller than a thousandth of the
/// tallest bar is not drawn.
const PORTIONS: u64 = 1000;

/// One piece of a bar: how much it adds up to, and which series it belongs to.
/// The series decides the color, see [`series_color`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarPart {
    pub portion: u64,
    pub series: usize,
}

/// What a series is called in the legend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegendEntry<'a> {
    pub name: &'a str,
    pub series: usize,
}

/// Three series have a color of their own; every further one is the dim text
/// color, which is where "Other" belongs.
pub fn series_color(series: usize, palette: &Palette) -> Color {
    match series {
        0 => palette.border,
        1 => palette.directory,
        2 => palette.menu_shortcut,
        _ => palette.dim_text,
    }
}

/// The heights of the pieces of one bar out of [`PORTIONS`], the empty space
/// above them first. `tallest` is the total of the highest bar.
fn heights(parts: &[BarPart], tallest: u64) -> (u16, Vec<(u16, usize)>) {
    let scale = |portion: u64| -> u16 {
        if tallest == 0 {
            return 0;
        }
        u16::try_from((portion * PORTIONS + tallest.div_euclid(2)).div_euclid(tallest))
            .unwrap_or(u16::MAX)
    };
    let pieces: Vec<(u16, usize)> = parts
        .iter()
        .map(|part| (scale(part.portion), part.series))
        .filter(|(height, _)| *height > 0)
        .collect();
    let filled: u64 = pieces.iter().map(|(height, _)| u64::from(*height)).sum();
    let empty = u16::try_from(PORTIONS.saturating_sub(filled)).unwrap_or(u16::MAX);
    (empty, pieces)
}

pub fn day_bars<'a, M: 'a>(
    bars: &[Vec<BarPart>],
    legend: &[LegendEntry<'_>],
    palette: &Palette,
) -> Element<'a, M> {
    let tallest = bars
        .iter()
        .map(|parts| parts.iter().map(|part| part.portion).sum::<u64>())
        .max()
        .unwrap_or(0);
    let columns = bars.iter().map(|parts| {
        let (empty, pieces) = heights(parts, tallest);
        let mut stack = Column::new().width(Length::Fill).height(BARS_HEIGHT);
        if empty > 0 {
            stack = stack.push(
                container("")
                    .width(Length::Fill)
                    .height(Length::FillPortion(empty)),
            );
        }
        for (height, series) in pieces {
            stack = stack.push(
                container("")
                    .width(Length::Fill)
                    .height(Length::FillPortion(height))
                    .style(theme::line(series_color(series, palette))),
            );
        }
        Element::from(stack)
    });
    let bars_row = Row::with_children(columns)
        .spacing(BAR_GAP)
        .width(Length::Fill)
        .height(BARS_HEIGHT);
    let key = Row::with_children(legend.iter().map(|entry| {
        row![
            container("")
                .width(SWATCH_SIDE)
                .height(SWATCH_SIDE)
                .style(theme::line(series_color(entry.series, palette))),
            text(entry.name.to_owned())
                .size(font_size::SMALL)
                .color(palette.dim_text)
                .wrapping(Wrapping::None),
        ]
        .spacing(spacing::XS)
        .align_y(alignment::Vertical::Center)
        .into()
    }))
    .spacing(spacing::M);
    column![bars_row, key].spacing(spacing::XS).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn part(portion: u64, series: usize) -> BarPart {
        BarPart { portion, series }
    }

    #[test]
    fn the_tallest_bar_fills_the_height_and_others_keep_their_ratio() {
        let (empty, pieces) = heights(&[part(60, 0), part(40, 1)], 100);
        assert_eq!(empty, 0);
        assert_eq!(pieces, vec![(600, 0), (400, 1)]);

        let (short_empty, short_pieces) = heights(&[part(25, 2)], 100);
        assert_eq!(short_empty, 750);
        assert_eq!(short_pieces, vec![(250, 2)]);
    }

    #[test]
    fn a_day_without_tokens_is_all_empty_space() {
        assert_eq!(heights(&[], 100), (1000, Vec::new()));
        assert_eq!(heights(&[part(0, 0)], 100), (1000, Vec::new()));
        assert_eq!(heights(&[], 0), (1000, Vec::new()));
    }

    #[test]
    fn three_series_have_roles_and_the_rest_is_dim() {
        let palette = Palette::default();
        assert_eq!(series_color(0, &palette), palette.border);
        assert_eq!(series_color(1, &palette), palette.directory);
        assert_eq!(series_color(2, &palette), palette.menu_shortcut);
        assert_eq!(series_color(3, &palette), palette.dim_text);
        assert_eq!(series_color(9, &palette), palette.dim_text);
    }
}
