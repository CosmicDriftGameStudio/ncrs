//! The rows of a filing plan: a file of the read folder with what happens to
//! it, a file that comes into a folder of the write folder, and the scrolling
//! pane they stand in.

use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, mouse_area, row, scrollable, text};
use iced::{alignment, Color, Element, Length, Padding};

use crate::format;
use crate::frame::{SCROLLBAR_MARGIN, SCROLLBAR_WIDTH};
use crate::kit::{list_row, Columns};
use crate::palette::Palette;
use crate::theme::{self, font_size, size, spacing};

/// Horizontal room one level of the tree takes; the same as in the tree rows.
const INDENT: f32 = 16.0;
/// The room a fold mark takes in the tree rows, so labels line up.
const MARK_WIDTH: f32 = 14.0;

/// A file of the read folder.
#[derive(Debug)]
pub struct PlanFileRow<M> {
    pub name: String,
    /// What happens to the file, one line.
    pub what: String,
    pub what_color: Color,
    /// A chip after `what`, like the tag the file gets.
    pub chip: Option<String>,
    pub chosen: bool,
    pub on_press: M,
}

/// The cells of `columns` are the name and what happens; the chip follows the
/// second one.
pub fn plan_file_row<'a, M: Clone + 'a>(
    file: PlanFileRow<M>,
    columns: &Columns<'_>,
    palette: &Palette,
) -> Element<'a, M> {
    let (name_color, what_color) = if file.chosen {
        (palette.cursor_text, palette.cursor_text)
    } else {
        (palette.text, file.what_color)
    };
    let chip_width = file.chip.as_ref().map_or(0.0, |chip| {
        format::estimated_width(chip, font_size::SMALL) + 4.0 * spacing::XS
    });
    let what_width = (columns.text_width(1) - chip_width).max(0.0);
    let what = text(format::fit_end(&file.what, what_width, font_size::BODY))
        .size(font_size::BODY)
        .color(what_color)
        .wrapping(Wrapping::None);
    let chip = file.chip.map(|label| {
        container(text(label).size(font_size::SMALL).wrapping(Wrapping::None))
            .padding([1.0, spacing::XS])
            .style(theme::chip(palette, file.chosen))
    });
    let what_cell = container(
        row![what]
            .push(chip)
            .spacing(spacing::XS)
            .align_y(alignment::Vertical::Center),
    )
    .width(Length::FillPortion(
        columns.portions.get(1).copied().unwrap_or(1),
    ))
    .clip(true);
    let line = row![columns.cell(0, &file.name, name_color), what_cell]
        .height(size::ROW)
        .align_y(alignment::Vertical::Center);
    list_row(line.into(), file.on_press, file.chosen, true, palette)
}

/// A file that comes into a folder: `+ new name`, and `← old name` dim behind
/// it when it is renamed.
#[derive(Debug)]
pub struct NewFileRow<M> {
    pub depth: usize,
    pub new_name: String,
    pub old_name: Option<String>,
    pub chosen: bool,
    pub on_press: M,
    pub on_double_press: M,
}

pub fn new_file_row<'a, M: Clone + 'a>(file: NewFileRow<M>, palette: &Palette) -> Element<'a, M> {
    let (ink, old_ink) = if file.chosen {
        (palette.cursor_text, palette.cursor_text)
    } else {
        (palette.text, palette.dim_text)
    };
    let line = row![
        container("").width(MARK_WIDTH),
        text(format!("+ {}", file.new_name))
            .size(font_size::BODY)
            .color(ink)
            .wrapping(Wrapping::None),
    ]
    .push(file.old_name.map(|old| {
        text(format!("\u{2190} {old}"))
            .size(font_size::BODY)
            .color(old_ink)
            .wrapping(Wrapping::None)
    }))
    .spacing(spacing::XS)
    .align_y(alignment::Vertical::Center);
    let content = container(line)
        .padding(Padding {
            left: spacing::CELL_PADDING_X + INDENT * file.depth as f32,
            ..Padding::ZERO
        })
        .clip(true)
        .center_y(Length::Fill);
    mouse_area(
        button(content)
            .on_press(file.on_press)
            .width(Length::Fill)
            .height(size::SIDEBAR_ITEM)
            .padding(0.0)
            .style(theme::menu_item(palette, file.chosen)),
    )
    .on_double_click(file.on_double_press)
    .into()
}

/// `field` in the place of a row of a tree at `depth`, to type a name in.
pub fn editing_row<'a, M: Clone + 'a>(depth: usize, field: Element<'a, M>) -> Element<'a, M> {
    container(field)
        .padding(Padding {
            left: spacing::CELL_PADDING_X + MARK_WIDTH + INDENT * depth as f32,
            right: spacing::CELL_PADDING_X,
            ..Padding::ZERO
        })
        .width(Length::Fill)
        .into()
}

/// `rows` in a vertical scrollable that fills the room.
pub fn pane_scroll<'a, M: 'a>(rows: Vec<Element<'a, M>>) -> Element<'a, M> {
    scrollable(column(rows))
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::new()
                .width(SCROLLBAR_WIDTH)
                .scroller_width(SCROLLBAR_WIDTH)
                .margin(SCROLLBAR_MARGIN)
                .spacing(0.0),
        ))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
