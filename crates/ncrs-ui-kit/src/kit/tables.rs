//! List rows, aligned columns and the scrolling list.

use iced::mouse::ScrollDelta;
use iced::widget::button as widget_button;
use iced::widget::text::Wrapping;
use iced::widget::{column, container, mouse_area, row, text, Column, Row, Space};
use iced::{alignment, Background, Color, Element, Length, Padding, Theme};

use crate::format;
use crate::frame::{AVERAGE_GLYPH_WIDTH, SCROLLBAR_MARGIN, SCROLLBAR_WIDTH};
use crate::palette::Palette;
use crate::theme::{self, font_size, size, spacing};

/// One row of a list; a click sends `on_press`. The row under the cursor is
/// filled while the list has the keyboard focus (`focused`), else only
/// outlined.
pub fn list_row<'a, M: Clone + 'a>(
    content: Element<'a, M>,
    on_press: M,
    selected: bool,
    focused: bool,
    palette: &Palette,
) -> Element<'a, M> {
    list_row_of_height(content, on_press, selected, focused, size::ROW, palette)
}

/// [`list_row`] for rows of more than one line.
pub fn list_row_of_height<'a, M: Clone + 'a>(
    content: Element<'a, M>,
    on_press: M,
    selected: bool,
    focused: bool,
    row_height: f32,
    palette: &Palette,
) -> Element<'a, M> {
    widget_button(content)
        .on_press(on_press)
        .width(Length::Fill)
        .padding([0.0, spacing::CELL_PADDING_X])
        .height(row_height)
        .style(theme::row(palette, false, selected, focused))
        .into()
}

/// Room kept right of a table for the bar of a scrolling list.
pub const SCROLL_LANE: f32 = SCROLLBAR_WIDTH + SCROLLBAR_MARGIN;

/// The columns of a table of text that must stay on one line: each takes a
/// share of the line, and a cell is cut to the pixels its column holds. The
/// width of a text is estimated, see [`format::estimated_width`].
#[derive(Debug)]
pub struct Columns<'p> {
    /// The relative widths of the columns.
    pub portions: &'p [u16],
    /// The characters of the whole line, at the average width of a glyph.
    pub line_chars: usize,
}

impl Columns<'_> {
    /// The pixels the cells of the line share: the line less the padding of a
    /// row and the lane of the scroll bar.
    fn cells_width(&self) -> f32 {
        (self.line_chars as f32 * AVERAGE_GLYPH_WIDTH - 2.0 * spacing::CELL_PADDING_X - SCROLL_LANE)
            .max(0.0)
    }

    /// The pixels the text of column `index` may take: the share of the
    /// column less the gap to the next one.
    pub fn text_width(&self, index: usize) -> f32 {
        let total: f32 = self.portions.iter().copied().map(f32::from).sum();
        let portion = self.portions.get(index).copied().map_or(0.0, f32::from);
        if total <= 0.0 {
            return 0.0;
        }
        (self.cells_width() * portion / total - spacing::M).max(0.0)
    }

    /// The text of column `index`, cut at the end where it is too long.
    pub fn cell<'a, M: Clone + 'a>(
        &self,
        index: usize,
        content: &str,
        color: Color,
    ) -> Element<'a, M> {
        self.cell_of(
            index,
            format::fit_end(content, self.text_width(index), font_size::BODY),
            color,
        )
    }

    fn cell_of<'a, M: Clone + 'a>(
        &self,
        index: usize,
        shown: String,
        color: Color,
    ) -> Element<'a, M> {
        self.aligned_cell_of(index, shown, color, alignment::Horizontal::Left)
    }

    fn aligned_cell_of<'a, M: Clone + 'a>(
        &self,
        index: usize,
        shown: String,
        color: Color,
        align: alignment::Horizontal,
    ) -> Element<'a, M> {
        let portion = self.portions.get(index).copied().unwrap_or(1);
        container(
            text(shown)
                .size(font_size::BODY)
                .color(color)
                .wrapping(Wrapping::None),
        )
        .width(Length::FillPortion(portion))
        .padding(Padding {
            right: spacing::M,
            ..Padding::ZERO
        })
        .align_x(align)
        .clip(true)
        .into()
    }

    /// The dimmed titles of the columns, one text line high: nothing selects
    /// them, so they need no list row height and stay close to the heading.
    pub fn header<'a, M: Clone + 'a>(&self, labels: &[&str], palette: &Palette) -> Element<'a, M> {
        let dimmed: Vec<(&str, Color)> = labels
            .iter()
            .map(|label| (*label, palette.dim_text))
            .collect();
        self.line(&dimmed, alignment::Horizontal::Left, Length::Shrink)
    }

    /// One row of the table, every cell in `color`.
    pub fn row<'a, M: Clone + 'a>(&self, cells: &[&str], color: Color) -> Element<'a, M> {
        let colored: Vec<(&str, Color)> = cells.iter().map(|cell| (*cell, color)).collect();
        self.row_colored(&colored)
    }

    /// One row of the table, each cell in its own color.
    pub fn row_colored<'a, M: Clone + 'a>(&self, cells: &[(&str, Color)]) -> Element<'a, M> {
        self.line(cells, alignment::Horizontal::Left, size::ROW.into())
    }

    /// One row of the table, every cell in `color`, the last column set
    /// against its right edge: a column of times or amounts.
    pub fn row_last_right<'a, M: Clone + 'a>(
        &self,
        cells: &[&str],
        color: Color,
    ) -> Element<'a, M> {
        let colored: Vec<(&str, Color)> = cells.iter().map(|cell| (*cell, color)).collect();
        self.line(&colored, alignment::Horizontal::Right, size::ROW.into())
    }

    fn line<'a, M: Clone + 'a>(
        &self,
        cells: &[(&str, Color)],
        last: alignment::Horizontal,
        height: Length,
    ) -> Element<'a, M> {
        let missing = self.portions.len().saturating_sub(cells.len());
        let last_index = self.portions.len().saturating_sub(1);
        let shown = cells
            .iter()
            .enumerate()
            .map(|(index, (content, color))| {
                let align = if index == last_index {
                    last
                } else {
                    alignment::Horizontal::Left
                };
                self.aligned_cell_of(
                    index,
                    format::fit_end(content, self.text_width(index), font_size::BODY),
                    *color,
                    align,
                )
            })
            .chain(
                (cells.len()..cells.len() + missing)
                    .map(|index| self.cell(index, "", Color::TRANSPARENT)),
            );
        Row::with_children(shown)
            .padding([0.0, spacing::CELL_PADDING_X])
            .height(height)
            .align_y(alignment::Vertical::Center)
            .into()
    }
}

/// The shortest the thumb of the bar of a list gets.
const MIN_THUMB: f32 = 12.0;

/// Where the thumb starts and how long it is on a track of `track` pixels, for
/// a window of `visible` rows at `offset` in a list of `len`.
fn thumb_extent(len: usize, visible: usize, offset: usize, track: f32) -> (f32, f32) {
    let thumb = (track * visible as f32 / len.max(1) as f32).clamp(MIN_THUMB.min(track), track);
    let scrollable = len.saturating_sub(visible).max(1) as f32;
    let top = (track - thumb) * (offset as f32 / scrollable).min(1.0);
    (top, thumb)
}

/// A table whose rows are a window into a longer list: the header, the rows
/// shown, and right of them a bar that shows where the window is. The wheel
/// over the rows (not over the header or the rest of the page) sends
/// `on_scroll`.
pub fn scroll_list<'a, M: Clone + 'a>(
    header: Element<'a, M>,
    rows: Vec<Element<'a, M>>,
    window: ListView,
    on_scroll: impl Fn(ScrollDelta) -> M + 'a,
    palette: &Palette,
) -> Element<'a, M> {
    scroll_list_of_height(header, rows, window, size::ROW, on_scroll, palette)
}

/// [`scroll_list`] for rows of `row_height` pixels, as made by
/// [`list_row_of_height`].
pub fn scroll_list_of_height<'a, M: Clone + 'a>(
    header: Element<'a, M>,
    rows: Vec<Element<'a, M>>,
    window: ListView,
    row_height: f32,
    on_scroll: impl Fn(ScrollDelta) -> M + 'a,
    palette: &Palette,
) -> Element<'a, M> {
    let shown = rows.len();
    let track = row_height * shown as f32;
    let list = mouse_area(
        container(Column::with_children(rows))
            .width(Length::Fill)
            .height(track)
            .clip(true),
    )
    .on_scroll(on_scroll);
    let lane = if window.len > window.visible {
        let (top, thumb) = thumb_extent(window.len, window.visible, window.offset, track);
        let (track_color, thumb_color) = (palette.border_inactive, palette.dim_text);
        let fill = |color: Color| {
            move |_theme: &Theme| container::Style {
                background: Some(Background::Color(color)),
                border: iced::Border {
                    radius: SCROLLBAR_WIDTH.into(),
                    ..iced::Border::default()
                },
                ..container::Style::default()
            }
        };
        container(column![
            Space::new().height(top),
            container("")
                .width(SCROLLBAR_WIDTH)
                .height(thumb)
                .style(fill(thumb_color)),
        ])
        .width(SCROLLBAR_WIDTH)
        .height(track)
        .style(fill(track_color))
    } else {
        container("").width(SCROLLBAR_WIDTH).height(track)
    };
    column![
        container(header).padding(Padding {
            right: SCROLL_LANE,
            ..Padding::ZERO
        }),
        row![
            list,
            container(lane).padding(Padding {
                left: SCROLLBAR_MARGIN,
                ..Padding::ZERO
            })
        ],
    ]
    .into()
}

/// Which rows of a list a window shows, for [`scroll_list`].
#[derive(Debug, Clone, Copy)]
pub struct ListView {
    pub len: usize,
    pub visible: usize,
    pub offset: usize,
}

// reason: a failing assertion is the signal in a test
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_column_holds_its_share_of_the_line_less_the_gap() {
        let columns = Columns {
            portions: &[1, 2, 1],
            line_chars: 80,
        };
        let cells = 80.0 * AVERAGE_GLYPH_WIDTH - 2.0 * spacing::CELL_PADDING_X - SCROLL_LANE;
        let gap = spacing::M;
        assert!((columns.text_width(0) - (cells / 4.0 - gap)).abs() < 0.01);
        assert!((columns.text_width(1) - (cells / 2.0 - gap)).abs() < 0.01);
        assert!(columns.text_width(1) > columns.text_width(0));
    }

    #[test]
    fn a_column_that_does_not_exist_or_a_line_without_columns_holds_nothing() {
        let columns = Columns {
            portions: &[1, 1],
            line_chars: 40,
        };
        assert_eq!(columns.text_width(5), 0.0);
        let none = Columns {
            portions: &[],
            line_chars: 40,
        };
        assert_eq!(none.text_width(0), 0.0);
    }

    #[test]
    fn a_narrow_line_does_not_underflow() {
        let columns = Columns {
            portions: &[1, 1],
            line_chars: 2,
        };
        assert_eq!(columns.text_width(0), 0.0);
    }

    #[test]
    fn the_thumb_follows_the_offset_and_never_leaves_the_track() {
        let track = 5.0 * size::ROW;
        let (top, thumb) = thumb_extent(2421, 5, 0, track);
        assert_eq!(top, 0.0);
        assert!(thumb >= MIN_THUMB);
        let (end_top, end_thumb) = thumb_extent(2421, 5, 2416, track);
        assert!((end_top + end_thumb - track).abs() < 0.01);
        let (middle, _) = thumb_extent(100, 10, 45, track);
        assert!(middle > 0.0 && middle < track);
    }
}
