//! The tick grid of the permissions dialog: rows of three-state checkboxes
//! under column headings, and the line with the octal number.

use iced::widget::{column, container, row};
use iced::{alignment, Element, Length};

use crate::kit::{self, Check};
use crate::palette::Palette;
use crate::theme::spacing;

/// Width of the column with the row labels.
const LABEL_WIDTH: f32 = 96.0;
/// Width of one column of boxes.
const CELL_WIDTH: f32 = 104.0;
/// Width of the octal field.
const OCTAL_WIDTH: f32 = 72.0;

/// What the grid shows: one heading per column, one label per row and one
/// state per box, `states[row][column]`.
pub struct Grid<'a, const ROWS: usize, const COLUMNS: usize> {
    pub headings: [&'a str; COLUMNS],
    pub labels: [&'a str; ROWS],
    pub states: [[Check; COLUMNS]; ROWS],
}

impl<const ROWS: usize, const COLUMNS: usize> std::fmt::Debug for Grid<'_, ROWS, COLUMNS> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Grid").finish_non_exhaustive()
    }
}

/// The grid. `on_toggle` gets the row and the column of the clicked box.
pub fn grid<'a, M: Clone + 'a, const ROWS: usize, const COLUMNS: usize>(
    shown: &Grid<'a, ROWS, COLUMNS>,
    on_toggle: impl Fn(usize, usize) -> M,
    palette: &Palette,
) -> Element<'a, M> {
    let mut headings = row![container(kit::dim(String::new(), palette)).width(LABEL_WIDTH)];
    for heading in shown.headings {
        headings = headings.push(
            container(kit::dim(heading.to_owned(), palette))
                .width(CELL_WIDTH)
                .align_x(alignment::Horizontal::Left),
        );
    }
    let mut rows = column![headings].spacing(spacing::LINE_GAP);
    for (row_index, (label, states)) in shown.labels.iter().zip(shown.states).enumerate() {
        let mut line =
            row![container(kit::paragraph((*label).to_owned(), palette)).width(LABEL_WIDTH)]
                .align_y(alignment::Vertical::Center);
        for (column_index, state) in states.into_iter().enumerate() {
            line = line.push(
                container(kit::checkbox(
                    state,
                    Some(on_toggle(row_index, column_index)),
                    palette,
                ))
                .width(CELL_WIDTH),
            );
        }
        rows = rows.push(line);
    }
    rows.into()
}

/// A single box with a label beside it, for the extra bits and the
/// read-only attribute.
pub fn labelled_box<'a, M: Clone + 'a>(
    label: &str,
    state: Check,
    on_toggle: M,
    palette: &Palette,
) -> Element<'a, M> {
    row![
        kit::checkbox(state, Some(on_toggle), palette),
        kit::paragraph(label.to_owned(), palette),
    ]
    .spacing(spacing::LINE_GAP)
    .align_y(alignment::Vertical::Center)
    .into()
}

/// The words of the octal line.
#[derive(Debug)]
pub struct OctalTexts<'a> {
    pub label: &'a str,
    pub value: &'a str,
    pub symbolic: &'a str,
    pub note: &'a str,
}

/// The line with the octal field: the label, the field, the rights spelled
/// like `ls -l` and a dimmed note.
pub fn octal_row<'a, M: Clone + 'a>(
    texts: &OctalTexts<'_>,
    field_id: &'static str,
    on_input: impl Fn(String) -> M + 'a,
    on_submit: M,
    palette: &Palette,
) -> Element<'a, M> {
    row![
        container(kit::paragraph(texts.label.to_owned(), palette)).width(LABEL_WIDTH),
        container(kit::text_field(
            "",
            texts.value,
            field_id,
            Some(on_input),
            on_submit,
            palette,
        ))
        .width(OCTAL_WIDTH),
        kit::dim(texts.symbolic.to_owned(), palette),
        container(kit::dim(texts.note.to_owned(), palette)).width(Length::Fill),
    ]
    .spacing(spacing::LINE_GAP)
    .align_y(alignment::Vertical::Center)
    .into()
}

/// A label in the column of the grid with `content` beside it, for the rows
/// under the grid that line up with it.
pub fn labelled_row<'a, M: Clone + 'a>(
    label: &str,
    content: Element<'a, M>,
    palette: &Palette,
) -> Element<'a, M> {
    row![
        container(kit::paragraph(label.to_owned(), palette)).width(LABEL_WIDTH),
        content,
    ]
    .spacing(spacing::LINE_GAP)
    .align_y(alignment::Vertical::Center)
    .into()
}
