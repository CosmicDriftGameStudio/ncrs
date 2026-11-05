//! A gauge of the System page: a title with its value, a bar and a detail line.

use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, text};
use iced::{Color, Element, Length};

use crate::palette::Palette;
use crate::theme::{self, font_size, spacing};

const WARNING_FROM_PERCENT: f32 = 50.0;
const CRITICAL_FROM_PERCENT: f32 = 90.0;
const BAR_HEIGHT: f32 = 8.0;
const PORTIONS: f32 = 1000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeterLevel {
    Normal,
    Warning,
    Critical,
}

/// The level a fill of `percent` percent is drawn in.
pub fn meter_level(percent: f32) -> MeterLevel {
    if percent >= CRITICAL_FROM_PERCENT {
        MeterLevel::Critical
    } else if percent >= WARNING_FROM_PERCENT {
        MeterLevel::Warning
    } else {
        MeterLevel::Normal
    }
}

/// The color a value of `level` is written in; the normal one is plain text.
pub fn level_text_color(level: MeterLevel, palette: &Palette) -> Color {
    match level {
        MeterLevel::Normal => palette.text,
        MeterLevel::Warning => palette.accent,
        MeterLevel::Critical => palette.error,
    }
}

fn level_color(level: MeterLevel, palette: &Palette) -> Color {
    match level {
        MeterLevel::Normal => palette.border,
        MeterLevel::Warning => palette.accent,
        MeterLevel::Critical => palette.error,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Meter<'a> {
    pub title: &'a str,
    pub value: &'a str,
    /// How full the bar is, `0.0` to `1.0`.
    pub fraction: f32,
    pub level: MeterLevel,
    pub detail: &'a str,
}

pub fn meter<'a, M: 'a>(meter: Meter<'_>, palette: &Palette) -> Element<'a, M> {
    let fill_color = level_color(meter.level, palette);
    let value_color = level_text_color(meter.level, palette);
    let filled = (meter.fraction.clamp(0.0, 1.0) * PORTIONS).round() as u16;
    let empty = PORTIONS as u16 - filled;
    let part = |portion: u16, tint: Color| {
        container("")
            .width(Length::FillPortion(portion))
            .height(BAR_HEIGHT)
            .style(theme::line(tint))
    };
    let mut bar = row![];
    if filled > 0 {
        bar = bar.push(part(filled, fill_color));
    }
    if empty > 0 {
        bar = bar.push(part(empty, palette.border_inactive));
    }
    let label = |content: &str, tint: Color| {
        text(content.to_owned())
            .size(font_size::BODY)
            .color(tint)
            .wrapping(Wrapping::None)
    };
    column![
        row![
            label(meter.title, palette.text).width(Length::Fill),
            label(meter.value, value_color),
        ],
        bar,
        label(meter.detail, palette.dim_text),
    ]
    .spacing(spacing::LINE_GAP)
    .width(Length::Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_change_at_fifty_and_ninety_percent() {
        assert_eq!(meter_level(0.0), MeterLevel::Normal);
        assert_eq!(meter_level(49.9), MeterLevel::Normal);
        assert_eq!(meter_level(50.0), MeterLevel::Warning);
        assert_eq!(meter_level(89.9), MeterLevel::Warning);
        assert_eq!(meter_level(90.0), MeterLevel::Critical);
        assert_eq!(meter_level(250.0), MeterLevel::Critical);
    }
}
