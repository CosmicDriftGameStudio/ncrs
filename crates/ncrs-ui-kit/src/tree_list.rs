//! The rows of a tree and of a list of search results in a sidebar: an
//! indented label with a fold mark, and a title with where it is and a snippet.

use std::sync::LazyLock;

use iced::widget::svg::{self, Svg};
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, text};
use iced::{alignment, Color, Element, Length, Padding, Theme};

use crate::page_scroll::page_scroll_padded;
use crate::palette::Palette;
use crate::theme::{self, font_size, size, spacing};

/// Horizontal room one level of the tree takes.
pub const TREE_INDENT: f32 = 10.0;
/// Room of the fold mark, so labels line up with and without one.
const MARK_WIDTH: f32 = 10.0;
/// Room of the status letter before a label that has one.
const LEAD_WIDTH: f32 = 14.0;
/// What a row takes before its label: the fold mark and the gap after it.
pub const LABEL_LEAD: f32 = MARK_WIDTH + spacing::XS;
/// What a tree row takes besides its label: left padding, mark and gap.
pub const ROW_OVERHEAD: f32 = spacing::CELL_PADDING_X + LABEL_LEAD;
/// The margin around the rows of a tree list; every tree sits in it alike.
pub const LIST_INSET: f32 = spacing::XS;
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
    /// A short fixed column before the label, such as the status letter of a
    /// changed file; it takes the ink of the cursor when the row is chosen.
    pub lead: Option<(String, Color)>,
    /// Small texts at the right end, such as line counts.
    pub trail: Vec<(String, Color)>,
    pub on_press: M,
}

/// `content` in a scrollable that fills the room, set in by [`LIST_INSET`].
pub fn tree_scroll<'a, M: 'a>(id: &'static str, content: Element<'a, M>) -> Element<'a, M> {
    page_scroll_padded(id, content, Padding::from(LIST_INSET))
}

pub fn tree_row<'a, M: Clone + 'a>(row_of: TreeRow<M>, palette: &Palette) -> Element<'a, M> {
    tree_row_with_lock(row_of, false, None, palette)
}

/// The size of the lock after the label of a locked row.
const LOCK_SIZE: f32 = 12.0;

/// A lock in the success color: the symbol font has no glyph that takes a color.
fn lock_mark<'a, M: 'a>(color: iced::Color) -> Element<'a, M> {
    static HANDLE: LazyLock<svg::Handle> = LazyLock::new(|| {
        svg::Handle::from_memory(
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="4" y="11" width="16" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/></svg>"##.to_vec(),
        )
    });
    Svg::new(LazyLock::force(&HANDLE).clone())
        .width(LOCK_SIZE)
        .height(LOCK_SIZE)
        .style(move |_: &Theme, _| svg::Style { color: Some(color) })
        .into()
}

/// [`tree_row`], with a green lock after the label when `locked` is set (an
/// encrypted note or folder). With `on_lock` a press on the lock sends it
/// instead of pressing the row.
pub fn tree_row_with_lock<'a, M: Clone + 'a>(
    row_of: TreeRow<M>,
    locked: bool,
    on_lock: Option<M>,
    palette: &Palette,
) -> Element<'a, M> {
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
    let cursor_ink = |color: Color| {
        if row_of.chosen {
            palette.cursor_text
        } else {
            color
        }
    };
    let mut line = row![container(text(mark).size(font_size::BODY).color(ink)).width(MARK_WIDTH)]
        .spacing(spacing::XS)
        .width(Length::Fill)
        .align_y(alignment::Vertical::Center);
    if let Some((letter, color)) = row_of.lead {
        line = line.push(
            container(text(letter).size(font_size::BODY).color(cursor_ink(color)))
                .width(LEAD_WIDTH),
        );
    }
    line = line.push(
        text(row_of.label)
            .size(font_size::BODY)
            .color(ink)
            .wrapping(Wrapping::None),
    );
    if locked {
        let lock = lock_mark(palette.success);
        line = line.push(match on_lock {
            Some(message) => Element::from(
                button(lock)
                    .on_press(message)
                    .padding(0.0)
                    .style(|_, _| button::Style::default()),
            ),
            None => lock,
        });
    }
    if !row_of.trail.is_empty() {
        line = line.push(container("").width(Length::Fill));
        for (shown, color) in row_of.trail {
            line = line.push(
                text(shown)
                    .size(font_size::SMALL)
                    .color(cursor_ink(color))
                    .wrapping(Wrapping::None),
            );
        }
    }
    let content = container(line)
        .padding(Padding {
            left: spacing::CELL_PADDING_X + TREE_INDENT * row_of.depth as f32,
            right: spacing::CELL_PADDING_X,
            ..Padding::ZERO
        })
        .clip(true)
        .center_y(Length::Fill);
    button(content)
        .on_press(row_of.on_press)
        .width(Length::Fill)
        .height(size::ROW)
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
