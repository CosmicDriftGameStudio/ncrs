//! The rows of a tree and of a list of search results in a sidebar: an
//! indented label with a fold mark, and a title with where it is and a snippet.

use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, text};
use iced::{alignment, Element, Length, Padding};

use crate::palette::Palette;
use crate::theme::{self, font_size, size, spacing};

/// Horizontal room one level of the tree takes.
const INDENT: f32 = 16.0;
/// Room of the fold mark, so labels line up with and without one.
const MARK_WIDTH: f32 = 14.0;
const HIT_GAP: f32 = 2.0;

pub const MARK_OPEN: &str = "\u{25be}";
pub const MARK_CLOSED: &str = "\u{25b8}";

#[derive(Debug)]
pub struct TreeRow<M> {
    pub depth: usize,
    /// `Some(true)` for a folder that is open, `Some(false)` for one that is
    /// closed, `None` for a leaf.
    pub open: Option<bool>,
    pub label: String,
    /// The row under the cursor.
    pub chosen: bool,
    /// Drawn in the dim ink, like a group that is made up.
    pub dim: bool,
    pub on_press: M,
}

pub fn tree_row<'a, M: Clone + 'a>(row_of: TreeRow<M>, palette: &Palette) -> Element<'a, M> {
    let ink = if row_of.chosen {
        palette.cursor_text
    } else if row_of.dim {
        palette.dim_text
    } else {
        palette.text
    };
    let mark = match row_of.open {
        Some(true) => MARK_OPEN,
        Some(false) => MARK_CLOSED,
        None => "",
    };
    let line = row![
        container(text(mark).size(font_size::BODY).color(ink)).width(MARK_WIDTH),
        text(row_of.label)
            .size(font_size::BODY)
            .color(ink)
            .wrapping(Wrapping::None),
    ]
    .spacing(spacing::XS)
    .align_y(alignment::Vertical::Center);
    let content = container(line)
        .padding(Padding {
            left: spacing::CELL_PADDING_X + INDENT * row_of.depth as f32,
            ..Padding::ZERO
        })
        .clip(true)
        .center_y(Length::Fill);
    button(content)
        .on_press(row_of.on_press)
        .width(Length::Fill)
        .height(size::SIDEBAR_ITEM)
        .padding(0.0)
        .style(theme::menu_item(palette, row_of.chosen))
        .into()
}

#[derive(Debug)]
pub struct ResultRow<M> {
    pub title: String,
    /// Where it is, dim, one line.
    pub place: String,
    pub snippet: String,
    pub chosen: bool,
    pub on_press: M,
}

pub fn result_row<'a, M: Clone + 'a>(result: ResultRow<M>, palette: &Palette) -> Element<'a, M> {
    let (title_ink, dim_ink) = if result.chosen {
        (palette.cursor_text, palette.cursor_text)
    } else {
        (palette.text, palette.dim_text)
    };
    let small = |content: String, ink| {
        text(content)
            .size(font_size::SMALL)
            .color(ink)
            .wrapping(Wrapping::None)
    };
    let lines = column![
        text(result.title)
            .size(font_size::BODY)
            .color(title_ink)
            .wrapping(Wrapping::None),
        small(result.place, dim_ink),
        small(result.snippet, title_ink),
    ]
    .spacing(HIT_GAP);
    button(
        container(lines)
            .padding([spacing::XS, spacing::CELL_PADDING_X])
            .clip(true),
    )
    .on_press(result.on_press)
    .width(Length::Fill)
    .padding(0.0)
    .style(theme::menu_item(palette, result.chosen))
    .into()
}
