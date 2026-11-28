//! The colors of the Platz tab and the button of its drive warning: what the bar of the drive and the rows are
//! tinted with. The names are the groups of the tab, so no feature picks a
//! hex value of its own.

use iced::widget::button;
use iced::{Background, Border, Color, Theme};

use super::{git, radius, spacing, HOVER_ALPHA};
use crate::palette::Palette;

/// What can be deleted for good because a tool makes it again.
pub fn safe(palette: &Palette) -> Color {
    palette.success
}

/// Merged worktrees.
pub const fn worktrees() -> Color {
    git::cyan()
}

/// Files of the user's own: the user decides.
pub const fn decide() -> Color {
    git::orange()
}

/// Folders a running program holds open.
pub fn in_use(palette: &Palette) -> Color {
    palette.dim_text
}

/// Everything else on the drive.
pub fn rest(palette: &Palette) -> Color {
    palette.dim_text.scale_alpha(0.45)
}

/// What is free on the drive; drawn like the ground, the bar outlines it.
pub fn free(palette: &Palette) -> Color {
    palette.background
}

/// The button of an action the warning is about: outlined and inked in the warning orange.
pub fn warning_outline_button(
    palette: &Palette,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    let orange = git::orange();
    move |_theme, status| {
        let disabled = matches!(status, button::Status::Disabled);
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: hovered.then(|| Background::Color(orange.scale_alpha(HOVER_ALPHA))),
            text_color: if disabled { colors.dim_text } else { orange },
            border: Border {
                color: if disabled {
                    colors.border_inactive
                } else {
                    orange
                },
                width: spacing::BORDER_WIDTH,
                radius: radius::CORNER.into(),
            },
            ..button::Style::default()
        }
    }
}
