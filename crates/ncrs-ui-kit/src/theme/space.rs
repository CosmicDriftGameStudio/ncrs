//! The colors of the Platz tab: what the bar of the drive and the rows are
//! tinted with. The names are the groups of the tab, so no feature picks a
//! hex value of its own.

use iced::Color;

use super::git;
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
