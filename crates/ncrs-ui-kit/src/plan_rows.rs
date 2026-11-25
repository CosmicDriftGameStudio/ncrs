//! The rows of a filing plan: a file of the read folder with what happens to
//! it, a file that comes into a folder of the write folder, and the scrolling
//! pane they stand in.

use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, mouse_area, row, scrollable, text, Row};
use iced::{alignment, Color, Element, Length, Padding};

use crate::format;
use crate::frame::{SCROLLBAR_MARGIN, SCROLLBAR_WIDTH};
use crate::kit::{list_row, SCROLL_LANE};
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

/// The titles of the two columns, over the rows.
pub fn plan_header<'a, M: 'a>(name: &str, what: &str, palette: &Palette) -> Element<'a, M> {
    let title = |label: &str| {
        text(label.to_owned())
            .size(font_size::BODY)
            .color(palette.dim_text)
            .wrapping(Wrapping::None)
    };
    container(row![
        container(title(name)).width(size::PLAN_NAME),
        title(what)
    ])
    .padding([0.0, spacing::CELL_PADDING_X])
    .height(size::COLUMN_HEADER)
    .width(Length::Fill)
    .align_y(alignment::Vertical::Center)
    .into()
}

/// A line over or under the rows, set in like them.
pub fn plan_side_line<'a, M: 'a>(content: Element<'a, M>) -> Element<'a, M> {
    container(content)
        .padding([PLAN_LINE_PADDING_Y, spacing::CELL_PADDING_X])
        .width(Length::Fill)
        .into()
}

const PLAN_LINE_PADDING_Y: f32 = 2.0;

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

/// A row of `line_width` pixels: the name, what happens, the chip and the links
/// at the right end.
pub fn plan_action_row<'a, M: Clone + 'a>(
    action: PlanActionRow<M>,
    line_width: f32,
    palette: &Palette,
) -> Element<'a, M> {
    if action.links.is_empty() {
        return plan_line(action.file, None, line_width, palette);
    }
    let chosen = action.file.chosen;
    let mut width = 0.0;
    let mut strip = Row::new().align_y(alignment::Vertical::Center);
    for (at, link) in action.links.into_iter().enumerate() {
        let is_marked = action.marked_link == Some(at);
        let padding = if is_marked { spacing::XS } else { 0.0 };
        width += spacing::M
            + 2.0 * padding
            + format::estimated_width(&link.label, font_size::SMALL);
        let ink = if is_marked {
            palette.accent
        } else if chosen {
            palette.cursor_text
        } else {
            palette.dim_text
        };
        let label = text(link.label)
            .size(font_size::SMALL)
            .color(ink)
            .wrapping(Wrapping::None);
        let shown = button(label)
            .padding([0.0, padding])
            .on_press(link.on_press)
            .style(theme::named_link_button(palette));
        // A marked link takes the ground of the window, or the mark would be
        // lost on the cursor fill of its row.
        let ground = if is_marked {
            palette.background
        } else {
            Color::TRANSPARENT
        };
        strip = strip.push(
            container(
                container(shown).style(move |_| container::background(ground)),
            )
            .padding(Padding {
                left: spacing::M,
                ..Padding::ZERO
            }),
        );
    }
    plan_line(action.file, Some((strip.into(), width)), line_width, palette)
}

/// `links` come with the pixels they take.
fn plan_line<'a, M: Clone + 'a>(
    file: PlanFileRow<M>,
    links: Option<(Element<'a, M>, f32)>,
    line_width: f32,
    palette: &Palette,
) -> Element<'a, M> {
    let (name_color, what_color) = if file.chosen {
        (palette.cursor_text, palette.cursor_text)
    } else {
        (palette.text, file.what_color)
    };
    let one_line = |content: String, color: Color| {
        text(content)
            .size(font_size::BODY)
            .color(color)
            .wrapping(Wrapping::None)
    };
    let name = container(one_line(
        format::fit_middle(&file.name, size::PLAN_NAME - spacing::M, font_size::BODY),
        name_color,
    ))
    .width(size::PLAN_NAME)
    .clip(true);
    let chip_width = file.chip.as_ref().map_or(0.0, |chip| {
        format::estimated_width(chip, font_size::SMALL) + 4.0 * spacing::XS
    });
    let links_width = links.as_ref().map_or(0.0, |(_, width)| *width);
    let what_width = line_width
        - 2.0 * spacing::CELL_PADDING_X
        - SCROLL_LANE
        - size::PLAN_NAME
        - chip_width
        - links_width;
    // A path keeps its end, a sentence its beginning.
    let cut = if file.what.contains(['/', '\\']) {
        format::fit_middle
    } else {
        format::fit_end
    };
    let what = one_line(
        cut(&file.what, what_width.max(0.0), font_size::BODY),
        what_color,
    );
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
    .width(Length::Fill)
    .clip(true);
    let mut line = row![name, what_cell]
        .height(size::ROW)
        .align_y(alignment::Vertical::Center);
    if let Some((after, _)) = links {
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
