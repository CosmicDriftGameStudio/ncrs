//! The rows of a filing plan: a file of the read folder with what happens to
//! it, a file that comes into a folder of the write folder, and the scrolling
//! pane they stand in.

use iced::widget::text::Wrapping;
use iced::widget::{column, container, mouse_area, row, scrollable, text};
use iced::{alignment, Color, Element, Length, Padding};

use crate::format;
use crate::frame::{SCROLLBAR_MARGIN, SCROLLBAR_WIDTH};
use crate::kit::{self, list_row, Columns};
use crate::palette::Palette;
use crate::theme::{self, font_size, size, spacing};
use crate::tree_list::{tree_row, TreeRow, LABEL_LEAD, TREE_INDENT};

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
    plan_line(file, None, columns, palette)
}

/// A link behind what happens to a file, like "Make a rule…".
#[derive(Debug)]
pub struct PlanLink<M> {
    pub label: String,
    pub on_press: M,
}

/// A [`PlanFileRow`] with links after it: what the user can do about the
/// file right there.
#[derive(Debug)]
pub struct PlanActionRow<M> {
    pub file: PlanFileRow<M>,
    pub links: Vec<PlanLink<M>>,
    /// The link the keyboard is on (Left and Right move it).
    pub marked_link: Option<usize>,
}

/// Like [`plan_file_row`]; the links stand at the right end of the row.
pub fn plan_action_row<'a, M: Clone + 'a>(
    action: PlanActionRow<M>,
    columns: &Columns<'_>,
    palette: &Palette,
) -> Element<'a, M> {
    let marked = action.marked_link;
    let chosen = action.file.chosen;
    let on_cursor = Palette {
        accent: palette.cursor_text,
        ..*palette
    };
    let links = action.links.into_iter().enumerate().map(|(at, link)| {
        // A marked link takes the ground of the window, or the mark would be
        // lost on the cursor fill of its row.
        let is_marked = marked == Some(at);
        let ink = if chosen && !is_marked { &on_cursor } else { palette };
        let shown = kit::text_link(link.label, link.on_press, ink);
        if is_marked {
            let ground = palette.background;
            container(shown)
                .padding([0.0, spacing::XS])
                .style(move |_| container::background(ground))
                .into()
        } else {
            shown
        }
    });
    let strip = row(links)
        .spacing(spacing::M)
        .align_y(alignment::Vertical::Center);
    let after_what = container(strip).padding(Padding {
        left: spacing::M,
        ..Padding::ZERO
    });
    plan_line(action.file, Some(after_what.into()), columns, palette)
}

fn plan_line<'a, M: Clone + 'a>(
    file: PlanFileRow<M>,
    links: Option<Element<'a, M>>,
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
    let mut line = row![columns.cell(0, &file.name, name_color), what_cell]
        .height(size::ROW)
        .align_y(alignment::Vertical::Center);
    if let Some(after) = links {
        line = line.push(after);
    }
    list_row(line.into(), file.on_press, file.chosen, true, palette)
}

/// The most room the old name behind a renamed file takes: the new name comes
/// first, a longer old name is cut with an ellipsis.
const OLD_NAME_WIDTH: f32 = 260.0;

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
    let row = tree_row(
        TreeRow {
            depth: file.depth,
            open: None,
            label: file.new_name,
            chosen: file.chosen,
            dim: false,
            lead: Some(("+".to_owned(), palette.text)),
            trail: file
                .old_name
                .map(|old| {
                    let shown = format!("\u{2190} {old}");
                    (
                        format::fit_end(&shown, OLD_NAME_WIDTH, font_size::SMALL),
                        palette.dim_text,
                    )
                })
                .into_iter()
                .collect(),
            on_press: file.on_press,
        },
        palette,
    );
    mouse_area(row).on_double_click(file.on_double_press).into()
}

/// `field` in the place of a row of a tree at `depth`, to type a name in.
pub fn editing_row<'a, M: Clone + 'a>(depth: usize, field: Element<'a, M>) -> Element<'a, M> {
    container(field)
        .padding(Padding {
            left: spacing::CELL_PADDING_X + LABEL_LEAD + TREE_INDENT * depth as f32,
            right: spacing::CELL_PADDING_X,
            ..Padding::ZERO
        })
        .width(Length::Fill)
        .into()
}

/// `row` as the one [`crate::row_scroll::row_into_view`] looks for with `row_id`.
pub fn marked_row<'a, M: 'a>(row: Element<'a, M>, row_id: &'static str) -> Element<'a, M> {
    container(row).id(row_id).into()
}

/// `rows` in a vertical scrollable with `scroll_id` that fills the room.
pub fn pane_scroll<'a, M: 'a>(
    scroll_id: &'static str,
    rows: Vec<Element<'a, M>>,
) -> Element<'a, M> {
    scrollable(column(rows))
        .id(scroll_id)
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
