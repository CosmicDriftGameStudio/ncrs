//! A table of fixed-height rows under a header with sort arrows: what the
//! System page lists its tasks in. The caller builds only the rows in view.

use iced::widget::text::Wrapping;
use iced::widget::{column, container, mouse_area, row, text, Row};
use iced::{alignment, Color, Element, Length};

use crate::layout::ROW_HEIGHT;
use crate::palette::Palette;
use crate::theme::{self, font_size, spacing};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMark {
    None,
    Descending,
    Ascending,
}

impl SortMark {
    fn suffix(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Descending => " \u{25BC}",
            Self::Ascending => " \u{25B2}",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TableColumn<M> {
    pub title: String,
    pub width: Length,
    pub align_right: bool,
    pub sort: SortMark,
    /// What a click on the header sends.
    pub on_press: Option<M>,
}

#[derive(Debug, Clone)]
pub struct TableCell {
    pub text: String,
    /// `None` draws the text in the color of its row.
    pub color: Option<Color>,
}

impl TableCell {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            color: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TableRow<M> {
    pub cells: Vec<TableCell>,
    pub is_cursor: bool,
    pub on_press: Option<M>,
    pub on_right_press: Option<M>,
}

fn aligned<'a, M: 'a>(content: Element<'a, M>, width: Length, right: bool) -> Element<'a, M> {
    container(content)
        .width(width)
        .clip(true)
        .align_x(if right {
            alignment::Horizontal::Right
        } else {
            alignment::Horizontal::Left
        })
        .into()
}

pub fn data_table<'a, M: Clone + 'a>(
    columns: &[TableColumn<M>],
    rows: Vec<TableRow<M>>,
    palette: &Palette,
) -> Element<'a, M> {
    let header_cells = columns.iter().fold(row![], |cells, column| {
        let active = column.sort != SortMark::None;
        let label = text(format!("{}{}", column.title, column.sort.suffix()))
            .size(font_size::COLUMN_HEADER)
            .color(if active {
                palette.accent
            } else {
                palette.dim_text
            })
            .wrapping(Wrapping::None);
        let cell = aligned(label.into(), column.width, column.align_right);
        cells.push(match &column.on_press {
            Some(press) => mouse_area(cell).on_press(press.clone()).into(),
            None => cell,
        })
    });
    let header = container(header_cells.spacing(spacing::M).height(ROW_HEIGHT))
        .padding([0.0, spacing::L])
        .align_y(alignment::Vertical::Center);

    let body = rows.into_iter().map(|table_row| {
        let text_color = if table_row.is_cursor {
            palette.cursor_text
        } else {
            palette.text
        };
        let cells: Row<'a, M> = columns
            .iter()
            .zip(table_row.cells)
            .map(|(column, cell)| {
                let label = text(cell.text)
                    .size(font_size::ROW)
                    .color(cell.color.unwrap_or(text_color))
                    .wrapping(Wrapping::None);
                aligned(label.into(), column.width, column.align_right)
            })
            .fold(row![], Row::push);
        let plain_line = container(cells.spacing(spacing::M).height(ROW_HEIGHT))
            .padding([0.0, spacing::L])
            .width(Length::Fill)
            .align_y(alignment::Vertical::Center);
        let line = if table_row.is_cursor {
            plain_line.style(theme::line(palette.cursor))
        } else {
            plain_line
        };
        let mut area = mouse_area(line);
        if let Some(press) = table_row.on_press {
            area = area.on_press(press);
        }
        if let Some(press) = table_row.on_right_press {
            area = area.on_right_press(press);
        }
        Element::from(area)
    });
    column![header, column(body)].into()
}
