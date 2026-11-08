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
            radius: radius::CORNER.into(),
        },
        ..container::Style::default()
    }
}

/// The row of a settings page that has the keys: a list row under the cursor,
/// filled across the full width.
pub fn focused_setting(palette: &Palette) -> impl Fn(&Theme) -> container::Style {
    let colors = *palette;
    move |_theme| container::Style {
        background: Some(Background::Color(colors.cursor)),
        text_color: Some(colors.cursor_text),
        ..container::Style::default()
    }
}

/// An entry of the feature list on the settings tab, a list row: the chosen
/// one is filled with the cursor while the list has the keys, and only
/// outlined in the cursor color while the keyboard is on the rows of its page
/// (`dimmed`).
pub fn feature_item(
    palette: &Palette,
    chosen: bool,
    dimmed: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let filled = chosen && !dimmed;
        let outlined = chosen && dimmed;
        let background = if filled {
            Some(Background::Color(colors.cursor))
        } else if hovered {
            Some(super::hover_fill(&colors))
        } else {
            None
        };
        button::Style {
            background,
            text_color: if filled {
                colors.cursor_text
            } else if chosen {
                colors.text
            } else {
                colors.dim_text
            },
            border: Border {
                color: if outlined {
                    colors.cursor
                } else {
                    Color::TRANSPARENT
                },
                width: spacing::BORDER_WIDTH,
                radius: 0.0.into(),
            },
            ..button::Style::default()
        }
    }
}
