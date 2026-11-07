//! The styles of the settings tab: the feature list, the row that has the
//! keyboard focus and the setting a command-list hit brought into view.

use iced::widget::{button, container};
use iced::{Background, Border, Color, Theme};

use super::{radius, spacing};
use crate::palette::Palette;

/// How strongly the chosen entry of the feature list is tinted.
const FEATURE_TINT: f32 = 0.2;

/// The setting a command-list hit brought into view: tinted and outlined like
/// the chosen entry of the feature list.
pub fn marked_setting(palette: &Palette) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| container::Style {
        background: Some(Background::Color(colors.cursor.scale_alpha(FEATURE_TINT))),
        border: Border {
            color: colors.cursor,
            width: spacing::BORDER_WIDTH,
            radius: radius::CONTROL.into(),
        },
        ..container::Style::default()
    }
}

/// The row of a settings page that has the keyboard focus: the accent colour
/// all round, like the stroke of the active tab.
pub fn focused_setting(palette: &Palette) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| container::Style {
        border: Border {
            color: colors.accent,
            width: FOCUS_FRAME_WIDTH,
            radius: radius::CONTROL.into(),
        },
        ..container::Style::default()
    }
}

const FOCUS_FRAME_WIDTH: f32 = 2.0;
/// The chosen page while the keyboard is on the rows: tinted, not outlined.
const FEATURE_TINT_DIMMED: f32 = 0.08;

/// An entry of the feature list on the settings tab; the chosen one is tinted
/// and outlined in the cursor color, or only faintly tinted while the keyboard
/// is on the rows of its page (`dimmed`).
pub fn feature_item(
    palette: &Palette,
    chosen: bool,
    dimmed: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let outlined = chosen && !dimmed;
        let background = if chosen {
            let tint = if dimmed {
                FEATURE_TINT_DIMMED
            } else {
                FEATURE_TINT
            };
            Some(Background::Color(colors.cursor.scale_alpha(tint)))
        } else if hovered {
            Some(Background::Color(colors.title))
        } else {
            None
        };
        button::Style {
            background,
            text_color: if chosen { colors.text } else { colors.dim_text },
            border: Border {
                color: if outlined {
                    colors.cursor
                } else {
                    Color::TRANSPARENT
                },
                width: spacing::BORDER_WIDTH,
                radius: radius::CONTROL.into(),
            },
            ..button::Style::default()
        }
    }
}
