//! The Compare column: the trailing cell of a row while two folders are
//! compared, and the words and colors of the marks that the compare bar and
//! the sync dialog share.
use iced::widget::{container, text};
use iced::{Color, Element};
use ncrs_core::CompareMark;
use ncrs_i18n::{Language, Msg};

use crate::layout::{HERMES_CELL_GAP, HERMES_COLUMN_WIDTH};
use crate::palette::Palette;
use crate::theme::font_size;

pub const fn mark_message(mark: CompareMark) -> Msg {
    match mark {
        CompareMark::OnlyHere => Msg::CompareMarkOnlyHere,
        CompareMark::NewerHere => Msg::CompareMarkNewerHere,
        CompareMark::OlderHere => Msg::CompareMarkOlderHere,
        CompareMark::Different => Msg::CompareMarkDifferent,
        CompareMark::Same => Msg::CompareMarkSame,
    }
}

/// Only and newer are the "on" role, different is plain text, the rest is dim.
pub fn mark_color(palette: &Palette, mark: CompareMark) -> Color {
    match mark {
        CompareMark::OnlyHere | CompareMark::NewerHere => palette.primary_button().0,
        CompareMark::Different => palette.text,
        CompareMark::OlderHere | CompareMark::Same => palette.dim_text,
    }
}

/// The cell of one row, [`HERMES_COLUMN_WIDTH`] wide like the Hermes cell it
/// replaces. `on_fill` draws the text in the row's own color, for the cursor row.
pub fn compare_cell<'a, M: 'a>(
    mark: CompareMark,
    on_fill: Option<Color>,
    lang: Language,
    palette: &Palette,
) -> Element<'a, M> {
    container(
        text(lang.text(mark_message(mark)))
            .size(font_size::SMALL)
            .color(on_fill.unwrap_or_else(|| mark_color(palette, mark)))
            .wrapping(text::Wrapping::None),
    )
    .padding(iced::Padding::ZERO.left(HERMES_CELL_GAP))
    .width(HERMES_COLUMN_WIDTH)
    .clip(true)
    .into()
}

/// What a row shows in the Compare column: no column outside a comparison,
/// an empty cell for a row without a mark (so the other columns keep their
/// place), or the mark.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CompareCell {
    #[default]
    Off,
    Blank,
    Mark(CompareMark),
}

pub fn blank_compare_cell<'a, M: 'a>() -> Element<'a, M> {
    container("").width(HERMES_COLUMN_WIDTH).into()
}
