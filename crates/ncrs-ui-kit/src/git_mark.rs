//! What git says about a file, as a panel row shows it: a one-glyph column
//! after the name, and the dimmed ink of an ignored file.

use iced::widget::{container, row, text};
use iced::{alignment, Color, Element, Length};

use crate::file_icon::git_branch_icon;
use crate::palette::Palette;
use crate::theme::{font_size, spacing};

/// The width of the Git column, one glyph.
pub const GIT_COLUMN_WIDTH: f32 = 18.0;
/// The size of the branch symbol in the column, its title and the legend.
pub const BRANCH_ICON_SIZE: f32 = 12.0;

/// The orange of a changed file and of the unsaved changes of a worktree.
pub const MODIFIED_ORANGE: Color = Color::from_rgb8(0xff, 0xb8, 0x6c);
/// The cyan of an untracked file and of the work in progress.
pub const UNTRACKED_CYAN: Color = Color::from_rgb8(0x8b, 0xe9, 0xfd);
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
    /// The folder is a repository of its own; drawn as the branch symbol.
    Repository,
}

impl GitMark {
    /// The letter of the state. A renamed file counts as modified before it
    /// gets here; a repository has the branch symbol instead of a letter.
    pub const fn glyph(self) -> Option<&'static str> {
        match self {
            Self::Modified => Some("M"),
            Self::Added => Some("A"),
            Self::Untracked => Some("?"),
            Self::Deleted => Some("D"),
            Self::Conflicted => Some("!"),
            Self::Repository | Self::Ignored => None,
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

/// What a row says about git: the mark in the Git column of its panel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GitCell {
    pub mark: Option<GitMark>,
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
    let content: Element<'a, M> = match mark {
        Some(GitMark::Repository) => git_branch_icon(
            on_cursor.unwrap_or_else(|| GitMark::Repository.color(palette)),
            BRANCH_ICON_SIZE,
        ),
        Some(found) => match found.glyph() {
            Some(glyph) => text(glyph)
                .size(font_size::FILE)
                .font(crate::APP_FONT)
                .color(on_cursor.unwrap_or_else(|| found.color(palette)))
                .wrapping(text::Wrapping::None)
                .into(),
            None => text("").size(font_size::FILE).into(),
        },
        None => text("").size(font_size::FILE).into(),
    };
    container(content)
        .width(GIT_COLUMN_WIDTH)
        .align_x(alignment::Horizontal::Center)
        .into()
}

/// The title of the Git column.
pub fn git_column_title<'a, M: 'a>(palette: &Palette) -> Element<'a, M> {
    git_branch_icon(palette.dim_text, BRANCH_ICON_SIZE)
}

/// A line of dim small text under a panel that explains the Git column. With
/// `with_branch_symbol` the branch symbol stands in front of it.
pub fn git_legend<'a, M: 'a>(
    line: String,
    with_branch_symbol: bool,
    palette: &Palette,
) -> Element<'a, M> {
    let words = text(line)
        .size(font_size::SMALL)
        .color(palette.dim_text)
        .font(crate::APP_FONT)
        .wrapping(text::Wrapping::None);
    let content = row![]
        .extend(with_branch_symbol.then(|| git_branch_icon(palette.dim_text, BRANCH_ICON_SIZE)))
        .push(words)
        .spacing(spacing::XS)
        .align_y(alignment::Vertical::Center);
    container(content)
        .padding([0.0, spacing::L])
        .width(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_has_its_glyph_and_repository_and_ignored_have_none() {
        assert_eq!(GitMark::Modified.glyph(), Some("M"));
        assert_eq!(GitMark::Added.glyph(), Some("A"));
        assert_eq!(GitMark::Untracked.glyph(), Some("?"));
        assert_eq!(GitMark::Deleted.glyph(), Some("D"));
        assert_eq!(GitMark::Conflicted.glyph(), Some("!"));
        assert_eq!(GitMark::Repository.glyph(), None);
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
