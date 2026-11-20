//! What git says about a file, as a panel row shows it: a one-glyph column
//! after the name, and the dimmed ink of an ignored file.

use iced::widget::{container, text};
use iced::{alignment, Color, Element, Length};

use crate::palette::Palette;
use crate::theme::font_size;

/// The width of the Git column, one glyph.
pub const GIT_COLUMN_WIDTH: f32 = 18.0;
/// The title of the Git column and the sign of a repository.
pub const REPOSITORY_SIGN: &str = "\u{2387}";

/// The orange of a changed file and of the unsaved changes of a worktree.
pub const MODIFIED_ORANGE: Color = Color::from_rgb8(0xff, 0xb8, 0x6c);
const UNTRACKED_CYAN: Color = Color::from_rgb8(0x8b, 0xe9, 0xfd);
/// How much of its ink an ignored file keeps.
const IGNORED_OPACITY: f32 = 0.5;

/// What the Git column says about a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitMark {
    Modified,
    Added,
    Untracked,
    Deleted,
    Conflicted,
    /// Shown in the row's ink, dimmed, and without a glyph.
    Ignored,
    /// The folder is a repository of its own.
    Repository,
}

impl GitMark {
    /// A renamed file counts as modified before it gets here.
    pub const fn glyph(self) -> Option<&'static str> {
        match self {
            Self::Modified => Some("M"),
            Self::Added => Some("A"),
            Self::Untracked => Some("?"),
            Self::Deleted => Some("D"),
            Self::Conflicted => Some("!"),
            Self::Repository => Some(REPOSITORY_SIGN),
            Self::Ignored => None,
        }
    }

    pub fn color(self, palette: &Palette) -> Color {
        match self {
            Self::Modified => MODIFIED_ORANGE,
            Self::Added => palette.success,
            Self::Untracked => UNTRACKED_CYAN,
            Self::Deleted | Self::Conflicted => palette.error,
            Self::Repository => palette.accent,
            Self::Ignored => palette.dim_text,
        }
    }
}

/// What a row says about git: the mark in the Git column of its panel, and
/// the sign after the name of a repository in a folder outside any.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GitCell {
    pub mark: Option<GitMark>,
    pub sign_after_name: bool,
}

/// The ink of a row whose file git ignores.
pub fn ignored_ink(ink: Color) -> Color {
    Color {
        a: ink.a * IGNORED_OPACITY,
        ..ink
    }
}

/// The cell of the Git column. On the cursor row the glyph takes the ink of
/// the row, so it stays readable on the cursor color.
pub fn git_cell<'a, M: 'a>(
    mark: Option<GitMark>,
    on_cursor: Option<Color>,
    palette: &Palette,
) -> Element<'a, M> {
    let glyph = mark.and_then(|mark| mark.glyph().map(|glyph| (glyph, mark.color(palette))));
    let content: Element<'a, M> = match glyph {
        Some((glyph, color)) => text(glyph)
            .size(font_size::FILE)
            .font(crate::APP_FONT)
            .color(on_cursor.unwrap_or(color))
            .wrapping(text::Wrapping::None)
            .into(),
        None => text("").size(font_size::FILE).into(),
    };
    container(content)
        .width(GIT_COLUMN_WIDTH)
        .align_x(alignment::Horizontal::Center)
        .into()
}

/// The sign after the name of a repository in a folder that is none.
pub fn repository_sign<'a, M: 'a>(color: Color) -> Element<'a, M> {
    text(REPOSITORY_SIGN)
        .size(font_size::SMALL)
        .font(crate::APP_FONT)
        .color(color)
        .width(Length::Shrink)
        .wrapping(text::Wrapping::None)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_has_its_glyph_and_an_ignored_file_has_none() {
        assert_eq!(GitMark::Modified.glyph(), Some("M"));
        assert_eq!(GitMark::Added.glyph(), Some("A"));
        assert_eq!(GitMark::Untracked.glyph(), Some("?"));
        assert_eq!(GitMark::Deleted.glyph(), Some("D"));
        assert_eq!(GitMark::Conflicted.glyph(), Some("!"));
        assert_eq!(GitMark::Repository.glyph(), Some(REPOSITORY_SIGN));
        assert_eq!(GitMark::Ignored.glyph(), None);
    }

    #[test]
    fn the_states_have_the_colors_of_the_design() {
        let palette = Palette::default();
        assert_eq!(GitMark::Modified.color(&palette), MODIFIED_ORANGE);
        assert_eq!(GitMark::Untracked.color(&palette), UNTRACKED_CYAN);
        assert_eq!(GitMark::Added.color(&palette), palette.success);
        assert_eq!(GitMark::Deleted.color(&palette), palette.error);
        assert_eq!(GitMark::Conflicted.color(&palette), palette.error);
    }

    #[test]
    fn an_ignored_ink_is_fainter() {
        let ink = Color::from_rgb(1.0, 1.0, 1.0);
        assert!(ignored_ink(ink).a < ink.a);
    }
}
