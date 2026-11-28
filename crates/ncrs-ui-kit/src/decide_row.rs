//! The pieces of the pane in which the user decides what of their own files goes to the trash:
//! the column titles, a group head with its tick, the rows of files, the "more" row, and the
//! narrow overview beside the list. All of them share the columns of [`FileColumns`].

use iced::widget::text::Wrapping;
use iced::widget::{column, container, mouse_area, row, text, Space};
use iced::{alignment, Color, Element, Length};

use crate::kit::{self, Check};
use crate::palette::Palette;
use crate::theme::git;
use crate::theme::{self, font_size, size, spacing};
use crate::APP_FONT_BOLD;

/// The tick, the name, the reason, the date, the size and the bar: the widths of the cells that a
/// row, a head and the column titles share, so the columns stay in line.
#[derive(Debug)]
pub struct FileColumns;

impl FileColumns {
    pub const TICK: f32 = 32.0;
    pub const NAME_PORTION: u16 = 3;
    pub const REASON_PORTION: u16 = 2;
    pub const DATE: f32 = 100.0;
    pub const SIZE: f32 = 80.0;
    pub const BAR: f32 = 120.0;
    const BAR_CELL: f32 = Self::BAR + spacing::XL;
    const BAR_HEIGHT: f32 = 8.0;
    const BAR_PORTIONS: u16 = 1000;
}

fn cell<'a, M: 'a>(content: impl Into<Element<'a, M>>, width: f32) -> Element<'a, M> {
    container(content).width(width).clip(true).into()
}

fn one_line<'a, M: 'a>(content: String, font: f32, ink: Color) -> Element<'a, M> {
    text(content)
        .size(font)
        .color(ink)
        .wrapping(Wrapping::None)
        .into()
}

fn bold_line<'a, M: 'a>(content: String, font: f32, ink: Color) -> Element<'a, M> {
    text(content)
        .size(font)
        .color(ink)
        .font(APP_FONT_BOLD)
        .wrapping(Wrapping::None)
        .into()
}

/// A bar of `fraction` of its width in `color`, on a faint track.
fn bar<'a, M: 'a>(fraction: f32, color: Color, width: f32, palette: &Palette) -> Element<'a, M> {
    let filled = if fraction <= 0.0 {
        0
    } else {
        ((fraction.clamp(0.0, 1.0) * f32::from(FileColumns::BAR_PORTIONS)).round() as u16)
            .max(FileColumns::BAR_PORTIONS.div_euclid(60))
    };
    let part = |portion: u16, tint: Color| {
        container("")
            .width(Length::FillPortion(portion))
            .height(FileColumns::BAR_HEIGHT)
            .style(theme::line(tint))
    };
    let mut line = row![];
    if filled > 0 {
        line = line.push(part(filled, color));
    }
    if filled < FileColumns::BAR_PORTIONS {
        line = line.push(part(
            FileColumns::BAR_PORTIONS - filled,
            palette.border_inactive.scale_alpha(0.4),
        ));
    }
    container(line).width(width).into()
}

/// The titles above the rows: name, reason, date and size.
pub fn file_columns_header<'a, M: 'a>(labels: [&str; 4], palette: &Palette) -> Element<'a, M> {
    let [name, reason, date, size_title] = labels;
    let title = |label: &str| one_line(label.to_uppercase(), font_size::SMALL, palette.dim_text);
    let colors = *palette;
    column![
        container(
            row![
                cell(Space::new(), FileColumns::TICK),
                container(title(name))
                    .width(Length::FillPortion(FileColumns::NAME_PORTION))
                    .clip(true),
                container(title(reason))
                    .width(Length::FillPortion(FileColumns::REASON_PORTION))
                    .clip(true),
                cell(title(date), FileColumns::DATE),
                container(title(size_title))
                    .width(FileColumns::SIZE)
                    .align_x(alignment::Horizontal::Right),
                Space::new().width(FileColumns::BAR_CELL),
            ]
            .spacing(spacing::M)
            .align_y(alignment::Vertical::Center),
        )
        .padding([0.0, spacing::XL])
        .height(size::COLUMN_HEADER)
        .align_y(alignment::Vertical::Center)
        .style(move |_theme: &iced::Theme| container::Style {
            background: Some(colors.background.into()),
            ..container::Style::default()
        }),
        theme::horizontal_rule(palette.border_inactive),
    ]
    .into()
}

pub struct GroupHead<M> {
    pub title: String,
    /// What the group means, dim after the title.
    pub hint: String,
    pub count: usize,
    pub size_text: String,
    /// The group against the biggest group, `0.0` to `1.0`.
    pub fraction: f32,
    pub color: Color,
    pub open: bool,
    pub check: Check,
    /// The group is not chosen: grey title and bar.
    pub dimmed: bool,
    /// The keys are on the head.
    pub focused: bool,
    pub on_toggle: M,
    pub on_check: M,
}

/// The head of a group of rows: fold arrow, tick for the whole group, title, hint, count, size
/// and a bar. A click folds, the tick chooses the group.
pub fn group_head<'a, M: Clone + 'a>(head: GroupHead<M>, palette: &Palette) -> Element<'a, M> {
    let colors = *palette;
    let focused = head.focused;
    let ink = if head.dimmed {
        palette.dim_text
    } else {
        palette.text
    };
    let arrow = if head.open { "\u{25be}" } else { "\u{25b8}" };
    let cells = row![
        container(
            row![
                one_line(arrow.to_owned(), font_size::SMALL, ink),
                kit::checkbox(head.check, Some(head.on_check), palette),
            ]
            .spacing(spacing::M)
            .align_y(alignment::Vertical::Center),
        )
        .width(FileColumns::TICK),
        container(
            row![
                bold_line(head.title.to_uppercase(), font_size::SMALL, ink),
                one_line(head.hint, font_size::SMALL, palette.dim_text),
            ]
            .spacing(spacing::M),
        )
        .width(Length::Fill)
        .clip(true),
        container(one_line(
            if head.count > 0 {
                head.count.to_string()
            } else {
                String::new()
            },
            font_size::SMALL,
            palette.dim_text,
        ))
        .width(FileColumns::DATE),
        container(bold_line(head.size_text, font_size::SMALL, ink))
            .width(FileColumns::SIZE)
            .align_x(alignment::Horizontal::Right),
        container(bar(
            head.fraction,
            if head.dimmed {
                palette.border_inactive
            } else {
                head.color
            },
            FileColumns::BAR,
            palette,
        ))
        .width(FileColumns::BAR_CELL),
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    let framed = column![
        theme::horizontal_rule(palette.border_inactive),
        container(cells)
            .padding([0.0, spacing::XL])
            .width(Length::Fill)
            .height(Length::Fill)
            .align_y(alignment::Vertical::Center)
            .style(move |theme: &iced::Theme| {
                if focused {
                    git::section_head_focused(&colors)(theme)
                } else {
                    git::section_head(&colors, false)(theme)
                }
            }),
        theme::horizontal_rule(palette.border_inactive),
    ];
    mouse_area(
        container(framed)
            .height(size::SECTION_HEAD)
            .width(Length::Fill),
    )
    .interaction(iced::mouse::Interaction::Pointer)
    .on_press(head.on_toggle)
    .into()
}

pub struct FileRow<M> {
    /// `None` for a row that cannot be ticked.
    pub check: Option<Check>,
    pub name: String,
    pub reason: String,
    pub date: String,
    pub size_text: String,
    pub fraction: f32,
    pub color: Color,
    /// Not chosen: name and size in the dim ink.
    pub dimmed: bool,
    /// The row of the cursor.
    pub cursor: bool,
    /// The cursor row while the list has the keys: outlined in the focus color.
    pub focused: bool,
    pub on_press: Option<M>,
    pub on_toggle: Option<M>,
}

fn row_frame<'a, M: Clone + 'a>(
    cells: Element<'a, M>,
    cursor: bool,
    focused: bool,
    on_press: Option<M>,
    palette: &Palette,
) -> Element<'a, M> {
    let colors = *palette;
    let framed = container(cells)
        .padding([0.0, spacing::XL])
        .width(Length::Fill)
        .height(size::ROW_TALL)
        .align_y(alignment::Vertical::Center)
        .style(move |theme: &iced::Theme| {
            let mut style = git::marked_row(&colors, cursor, false)(theme);
            if cursor && focused {
                style.border = theme::focus_border(&colors);
            }
            style
        });
    match on_press {
        Some(message) => mouse_area(framed).on_press(message).into(),
        None => framed.into(),
    }
}

/// One file or folder: tick, name, reason, date, size and a bar against the biggest row.
pub fn file_row<'a, M: Clone + 'a>(item: FileRow<M>, palette: &Palette) -> Element<'a, M> {
    let on_cursor = item.cursor;
    let (ink, dim_ink) = if on_cursor {
        (palette.cursor_text, palette.cursor_text)
    } else if item.dimmed {
        (palette.dim_text, palette.dim_text)
    } else {
        (palette.text, palette.dim_text)
    };
    let tick: Element<'a, M> = match item.check {
        Some(state) => kit::checkbox(state, item.on_toggle, palette),
        None => Space::new().into(),
    };
    let tint = if item.dimmed {
        palette.border_inactive
    } else {
        item.color
    };
    let cells = row![
        cell(tick, FileColumns::TICK),
        container(one_line(item.name, font_size::BODY, ink))
            .width(Length::FillPortion(FileColumns::NAME_PORTION))
            .clip(true),
        container(one_line(item.reason, font_size::SMALL, dim_ink))
            .width(Length::FillPortion(FileColumns::REASON_PORTION))
            .clip(true),
        cell(
            one_line(item.date, font_size::SMALL, dim_ink),
            FileColumns::DATE
        ),
        container(one_line(item.size_text, font_size::BODY, ink))
            .width(FileColumns::SIZE)
            .align_x(alignment::Horizontal::Right),
        container(bar(item.fraction, tint, FileColumns::BAR, palette)).width(FileColumns::BAR_CELL),
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    row_frame(
        cells.into(),
        on_cursor,
        item.focused,
        item.on_press,
        palette,
    )
}

/// The row that ends a long group: "5 more", their size, and a click opens them.
pub fn more_row<'a, M: Clone + 'a>(
    text_line: String,
    size_text: String,
    cursor: bool,
    focused: bool,
    on_press: M,
    palette: &Palette,
) -> Element<'a, M> {
    let ink = if cursor {
        palette.cursor_text
    } else {
        palette.dim_text
    };
    let cells = row![
        cell(Space::new(), FileColumns::TICK),
        container(one_line(text_line, font_size::SMALL, ink))
            .width(Length::Fill)
            .clip(true),
        container(one_line(size_text, font_size::SMALL, ink))
            .width(FileColumns::SIZE)
            .align_x(alignment::Horizontal::Right),
        Space::new().width(FileColumns::BAR_CELL),
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    row_frame(cells.into(), cursor, focused, Some(on_press), palette)
}

/// The head of a group in the narrow overview: title and the size of the group.
pub fn side_head<'a, M: 'a>(title: &str, size_text: String, palette: &Palette) -> Element<'a, M> {
    let colors = *palette;
    column![
        theme::horizontal_rule(palette.border_inactive),
        container(
            row![
                bold_line(title.to_uppercase(), font_size::SMALL, palette.text),
                Space::new().width(Length::Fill),
                bold_line(size_text, font_size::SMALL, palette.text),
            ]
            .align_y(alignment::Vertical::Center),
        )
        .padding([0.0, spacing::XL])
        .width(Length::Fill)
        .height(size::SECTION_HEAD)
        .align_y(alignment::Vertical::Center)
        .style(move |_theme: &iced::Theme| container::Style {
            background: Some(colors.background.into()),
            ..container::Style::default()
        }),
    ]
    .into()
}

/// How the overview row relates to the pane that is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideLook {
    Normal,
    /// The folder the pane shows.
    Current,
    /// Left alone by "delete all safe ones" or not offered: dim.
    Dim,
}

pub struct SideRow<M> {
    pub name: String,
    pub size_text: String,
    pub fraction: f32,
    pub color: Color,
    pub look: SideLook,
    /// The row of the overview keys, outlined while the overview has the keys.
    pub focused: bool,
    /// `None` for a row that cannot be opened here.
    pub on_press: Option<M>,
}

const SIDE_BAR: f32 = 48.0;
/// How many characters of a name fit in the overview before it is shortened with a dot row.
const SIDE_NAME_CHARS: usize = 17;

/// A row of the narrow overview: name, a small bar and the size.
pub fn side_row<'a, M: Clone + 'a>(item: SideRow<M>, palette: &Palette) -> Element<'a, M> {
    let current = item.look == SideLook::Current;
    let ink = match item.look {
        SideLook::Dim => palette.dim_text,
        SideLook::Current | SideLook::Normal => palette.text,
    };
    let tint = if item.look == SideLook::Dim {
        palette.border_inactive
    } else {
        item.color
    };
    let shown = crate::format::ellipsize_end(&item.name, SIDE_NAME_CHARS);
    let name = if current {
        bold_line(shown, font_size::BODY, palette.cursor_text)
    } else {
        one_line(shown, font_size::BODY, ink)
    };
    let size_ink = if current {
        palette.cursor_text
    } else {
        palette.dim_text
    };
    let cells = row![
        container(name).width(Length::Fill).clip(true),
        bar(item.fraction, tint, SIDE_BAR, palette),
        container(one_line(item.size_text, font_size::SMALL, size_ink))
            .width(FileColumns::SIZE - spacing::XL)
            .align_x(alignment::Horizontal::Right),
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    let colors = *palette;
    let focused = item.focused;
    let framed = container(cells)
        .padding([0.0, spacing::XL])
        .width(Length::Fill)
        .height(size::ROW_TALL)
        .align_y(alignment::Vertical::Center)
        .style(move |theme: &iced::Theme| {
            let mut style = git::marked_row(&colors, current, false)(theme);
            if focused {
                style.border = theme::focus_border(&colors);
            }
            style
        });
    match item.on_press {
        Some(message) => mouse_area(framed).on_press(message).into(),
        None => framed.into(),
    }
}

impl<M> std::fmt::Debug for GroupHead<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GroupHead")
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

impl<M> std::fmt::Debug for FileRow<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileRow")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl<M> std::fmt::Debug for SideRow<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SideRow")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}
