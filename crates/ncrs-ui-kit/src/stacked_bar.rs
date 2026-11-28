//! One horizontal bar cut into colored parts, like the fill of a drive.

use iced::widget::{container, row};
use iced::{Color, Element, Length};

use crate::palette::Palette;
use crate::theme::{self, size};

const PORTIONS: u64 = 10_000;
/// How many stripes a striped part is cut into.
const STRIPES: u16 = 40;

/// One part of the bar: its share is `portion` of the sum of all parts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarSegment {
    pub portion: u64,
    pub color: Color,
}

/// The widths of the parts in `FillPortion`s, in the order given. A part that
/// is not empty keeps at least one portion so it stays visible.
pub fn segment_portions(segments: &[BarSegment]) -> Vec<u16> {
    let total: u64 = segments.iter().map(|segment| segment.portion).sum();
    if total == 0 {
        return vec![0; segments.len()];
    }
    segments
        .iter()
        .map(|segment| match segment.portion {
            0 => 0,
            share => u16::try_from(share.saturating_mul(PORTIONS) / total)
                .unwrap_or(u16::MAX)
                .max(1),
        })
        .collect()
}

pub fn stacked_bar<'a, M: 'a>(segments: &[BarSegment], palette: &Palette) -> Element<'a, M> {
    stacked_bar_striped(segments, None, palette)
}

/// As [`stacked_bar`], with the part at `striped` cut into stripes: what was just freed.
pub fn stacked_bar_striped<'a, M: 'a>(
    segments: &[BarSegment],
    striped: Option<usize>,
    palette: &Palette,
) -> Element<'a, M> {
    let part = |portion: u16, color: Color| {
        container("")
            .width(Length::FillPortion(portion))
            .height(size::STACKED_BAR)
            .style(theme::line(color))
    };
    let mut bar = row![];
    for (place, (segment, portion)) in segments.iter().zip(segment_portions(segments)).enumerate() {
        if portion == 0 {
            continue;
        }
        if striped == Some(place) {
            let stripe = (portion / STRIPES).max(1);
            let mut left = portion;
            let mut solid = true;
            while left > 0 {
                let width = stripe.min(left);
                bar = bar.push(part(
                    width,
                    if solid {
                        segment.color
                    } else {
                        segment.color.scale_alpha(0.3)
                    },
                ));
                left -= width;
                solid = !solid;
            }
        } else {
            bar = bar.push(part(portion, segment.color));
        }
    }
    let edge = palette.border_inactive;
    container(bar)
        .width(Length::Fill)
        .style(move |_theme: &iced::Theme| container::Style {
            border: iced::Border {
                color: edge,
                width: theme::spacing::BORDER_WIDTH,
                radius: theme::radius::CORNER.into(),
            },
            ..container::Style::default()
        })
        .clip(true)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(portion: u64) -> BarSegment {
        BarSegment {
            portion,
            color: Color::WHITE,
        }
    }

    #[test]
    fn the_portions_follow_the_shares() {
        let portions = segment_portions(&[segment(1), segment(3)]);
        assert_eq!(portions, vec![2500, 7500]);
    }

    #[test]
    fn a_tiny_part_stays_visible_and_an_empty_one_is_left_out() {
        let portions = segment_portions(&[segment(1), segment(0), segment(1_000_000_000)]);
        assert_eq!(portions[0], 1);
        assert_eq!(portions[1], 0);
    }

    #[test]
    fn nothing_to_show_is_no_portion() {
        assert_eq!(segment_portions(&[segment(0), segment(0)]), vec![0, 0]);
    }
}
