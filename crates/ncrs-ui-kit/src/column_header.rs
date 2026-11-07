//! The column titles above the rows of a file list, and the layout of columns
//! the rows and the titles share.
use iced::mouse::Interaction;
use iced::widget::{
    button, column, container, mouse_area, opaque, pin, stack, text, Column as MenuColumn, Row,
    Space,
};
use iced::{alignment, Background, Element, Length, Point, Theme};
use ncrs_core::{Column, DateFormat, SizeFormat, SortKey};
use ncrs_i18n::{Language, Msg};

use crate::layout::{
    standard_column_width, COLUMN_DROP_LINE_WIDTH, COLUMN_GRIP_WIDTH, COLUMN_HEADER_HEIGHT,
    HERMES_CELL_GAP, HERMES_COLUMN_WIDTH, ICON_COLUMN_WIDTH, MAX_COLUMN_WIDTH, MENU_ITEM_HEIGHT,
    MENU_PADDING, MENU_SEPARATOR_HEIGHT, MENU_WIDTH, MIN_COLUMN_WIDTH, TAG_COLUMN_WIDTH,
};
use crate::palette::Palette;
use crate::theme::{self, font_size, spacing};

/// Room for the check mark in front of a menu entry.
const MENU_CHECK_WIDTH: f32 = 18.0;

/// Which columns a list shows, how wide they are and how their values are
/// written. The rows and the titles are built from the same one, so they stay
/// in line.
#[derive(Debug, Clone, Copy)]
pub struct ColumnLayout<'a> {
    /// In display order. Columns this system cannot show are skipped.
    pub columns: &'a [Column],
    /// Pixels; a column without an entry has its standard width.
    pub widths: &'a [(Column, u16)],
    pub date_format: DateFormat,
    pub size_format: SizeFormat,
    pub lang: Language,
}

impl ColumnLayout<'static> {
    /// Symbol, name, size and date in the formats the lists had before they
    /// were settings.
    pub const fn standard(lang: Language) -> Self {
        Self {
            columns: &Column::DEFAULT,
            widths: &[],
            date_format: DateFormat::Iso,
            size_format: SizeFormat::Readable,
            lang,
        }
    }
}

impl<'a> ColumnLayout<'a> {
    pub fn shown(self) -> impl Iterator<Item = Column> + 'a {
        self.columns
            .iter()
            .copied()
            .filter(|column| column.on_this_system())
    }

    pub fn width_of(&self, column: Column) -> f32 {
        if column == Column::Icon {
            return ICON_COLUMN_WIDTH;
        }
        self.widths
            .iter()
            .find(|(wide, _)| *wide == column)
            .map_or_else(
                || standard_column_width(column),
                |(_, width)| f32::from(*width),
            )
            .clamp(MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH)
    }

    /// What the columns besides the name take.
    pub fn fixed_width(self) -> f32 {
        self.shown()
            .filter(|column| *column != Column::Name)
            .map(|column| self.width_of(column))
            .sum()
    }
}

/// Where a value sits in its column: numbers and dates at the right.
pub const fn column_alignment(column: Column) -> alignment::Horizontal {
    match column {
        Column::Size | Column::Modified | Column::Created | Column::Accessed => {
            alignment::Horizontal::Right
        }
        _ => alignment::Horizontal::Left,
    }
}

/// The title of a column; the symbol column shows none in the header, the
/// menu names it.
pub const fn column_message(column: Column) -> Msg {
    match column {
        Column::Icon => Msg::ColumnIcon,
        Column::Name => Msg::ColumnName,
        Column::Size => Msg::ColumnSize,
        Column::Modified => Msg::ColumnModified,
        Column::Created => Msg::ColumnCreated,
        Column::Type => Msg::ColumnType,
        Column::Extension => Msg::ColumnExtension,
        Column::Permissions => Msg::ColumnPermissions,
        Column::Owner => Msg::ColumnOwner,
        Column::Group => Msg::ColumnGroup,
        Column::Accessed => Msg::ColumnAccessed,
        Column::LinkTarget => Msg::ColumnLinkTarget,
    }
}

/// Where a column lies in a list `panel_width` wide, counted from the left
/// edge of the panel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColumnSpan {
    pub column: Column,
    pub left: f32,
    pub width: f32,
}

/// The spans of the shown columns. The name takes what the others leave.
pub fn column_spans(
    layout: ColumnLayout<'_>,
    panel_width: f32,
    with_hermes: bool,
) -> Vec<ColumnSpan> {
    let hermes = if with_hermes {
        HERMES_COLUMN_WIDTH
    } else {
        0.0
    };
    let name_width = (panel_width
        - 2.0 * spacing::CELL_PADDING_X
        - TAG_COLUMN_WIDTH
        - layout.fixed_width()
        - hermes)
        .max(MIN_COLUMN_WIDTH);
    let mut left = spacing::CELL_PADDING_X + TAG_COLUMN_WIDTH;
    layout
        .shown()
        .map(|column| {
            let width = if column == Column::Name {
                name_width
            } else {
                layout.width_of(column)
            };
            let span = ColumnSpan {
                column,
                left,
                width,
            };
            left += width;
            span
        })
        .collect()
}

/// A title being dragged, as the header draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HeaderDrag {
    /// The title follows the pointer; the line shows where it would land,
    /// before the shown column with index `drop_before`.
    Reorder {
        column: Column,
        /// Left edge of the title, counted from the left edge of the panel.
        ghost_x: f32,
        drop_before: usize,
    },
    /// The right edge of the column follows the pointer.
    Resize { column: Column, width: f32 },
}

/// What the titles send. A title is pressed rather than clicked, because
/// press, drag and release all mean something on it.
pub struct HeaderEvents<'a, M> {
    pub on_press: Box<dyn Fn(Column) -> M + 'a>,
    pub on_resize: Box<dyn Fn(Column) -> M + 'a>,
    /// A right click anywhere on the header.
    pub on_context: M,
}

impl<M> std::fmt::Debug for HeaderEvents<'_, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeaderEvents").finish_non_exhaustive()
    }
}

/// The column titles. With `events` they can be pressed, dragged to another
/// place and resized at their right edge; the sorted one carries an arrow for
/// the direction. Without, they are plain text.
pub fn column_header<'a, M: Clone + 'a>(
    palette: &Palette,
    layout: ColumnLayout<'a>,
    sort: Option<SortKey>,
    with_hermes: bool,
    drag: Option<HeaderDrag>,
    events: Option<HeaderEvents<'a, M>>,
) -> Element<'a, M> {
    let lang = layout.lang;
    let title_of = |column: Column| {
        let arrow = match sort.filter(|key| column.sort_column() == Some(key.column)) {
            None => "",
            Some(key) if key.descending => " ▼",
            Some(_) => " ▲",
        };
        format!("{}{arrow}", lang.text(column_message(column)))
    };
    let width_of = |column: Column| match drag {
        Some(HeaderDrag::Resize {
            column: dragged,
            width,
        }) if dragged == column => width,
        _ => layout.width_of(column),
    };
    let shown: Vec<Column> = layout.shown().collect();

    let mut cells: Vec<Element<'a, M>> = Vec::with_capacity(shown.len() + 1);
    for (index, column) in shown.iter().copied().enumerate() {
        if matches!(drag, Some(HeaderDrag::Reorder { drop_before, .. }) if drop_before == index) {
            cells.push(drop_line(palette));
        }
        let title = if column == Column::Icon {
            String::new()
        } else {
            title_of(column)
        };
        let dimmed =
            matches!(drag, Some(HeaderDrag::Reorder { column: dragged, .. }) if dragged == column);
        let length = if column == Column::Name {
            Length::Fill
        } else {
            Length::Fixed(width_of(column))
        };
        let label = text(title)
            .size(font_size::SMALL)
            .color(if dimmed {
                palette.menu_disabled
            } else {
                palette.dim_text
            })
            .wrapping(text::Wrapping::None)
            .width(Length::Fill)
            .align_x(column_alignment(column));
        let cell = container(label)
            .width(length)
            .height(Length::Fill)
            .clip(true);
        let Some(handlers) = events.as_ref().filter(|_| column != Column::Icon) else {
            cells.push(cell.into());
            continue;
        };
        let pressable = mouse_area(cell)
            .on_press((handlers.on_press)(column))
            .interaction(Interaction::Pointer);
        if column == Column::Name {
            cells.push(pressable.into());
            continue;
        }
        let grip = mouse_area(Space::new().width(COLUMN_GRIP_WIDTH).height(Length::Fill))
            .on_press((handlers.on_resize)(column))
            .interaction(Interaction::ResizingHorizontally);
        cells.push(
            stack![
                pressable,
                container(grip)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(alignment::Horizontal::Right)
            ]
            .width(length)
            .height(Length::Fill)
            .into(),
        );
    }
    if matches!(drag, Some(HeaderDrag::Reorder { drop_before, .. }) if drop_before >= shown.len()) {
        cells.push(drop_line(palette));
    }

    let titles = Row::new()
        .push(Space::new().width(TAG_COLUMN_WIDTH))
        .extend(cells)
        .extend(with_hermes.then(|| {
            container(
                text(lang.text(Msg::ColumnHermes))
                    .size(font_size::SMALL)
                    .color(palette.dim_text)
                    .wrapping(text::Wrapping::None),
            )
            .padding(iced::Padding::ZERO.left(HERMES_CELL_GAP))
            .width(HERMES_COLUMN_WIDTH)
            .into()
        }))
        .align_y(alignment::Vertical::Center);
    let titles_bar: Element<'a, M> = container(titles)
        .padding([0.0, spacing::CELL_PADDING_X])
        .height(Length::Fill)
        .align_y(alignment::Vertical::Center)
        .width(Length::Fill)
        .into();

    let bar = match drag {
        Some(HeaderDrag::Reorder {
            column, ghost_x, ..
        }) => {
            let ghost = container(
                text(lang.text(column_message(column)))
                    .size(font_size::SMALL)
                    .color(palette.accent)
                    .wrapping(text::Wrapping::None),
            )
            .padding([1.0, spacing::XS])
            .style(theme::outlined_in(palette.accent));
            stack![
                titles_bar,
                pin(ghost)
                    .x(ghost_x)
                    .y(spacing::XS)
                    .width(Length::Fill)
                    .height(Length::Fill)
            ]
            .into()
        }
        _ => titles_bar,
    };

    let header = column![bar, theme::horizontal_rule(palette.border_inactive)]
        .height(COLUMN_HEADER_HEIGHT)
        .width(Length::Fill);
    match events {
        Some(handlers) => mouse_area(header)
            .on_right_press(handlers.on_context)
            .into(),
        None => header.into(),
    }
}

fn drop_line<'a, M: 'a>(palette: &Palette) -> Element<'a, M> {
    container(Space::new())
        .width(COLUMN_DROP_LINE_WIDTH)
        .height(Length::Fill)
        .style(theme::line(palette.accent))
        .into()
}

/// One line of the menu over the header.
#[derive(Debug, Clone)]
pub enum ColumnMenuItem<M> {
    Entry {
        label: String,
        checked: bool,
        /// Small text at the right, such as "always".
        note: Option<String>,
        /// `None` draws the entry as disabled.
        on_press: Option<M>,
    },
    Separator,
}

/// The height of a column menu with `entries` entries and `separators`
/// separators, for keeping it inside the window.
pub fn column_menu_height(entries: usize, separators: usize) -> f32 {
    entries as f32 * MENU_ITEM_HEIGHT
        + separators as f32 * MENU_SEPARATOR_HEIGHT
        + 2.0 * MENU_PADDING
}

/// The menu a right click on the header opens, laid over the window at
/// `origin`. A click beside it sends `on_close`.
pub fn column_menu<'a, M: Clone + 'a>(
    items: Vec<ColumnMenuItem<M>>,
    origin: Point,
    on_close: M,
    palette: &Palette,
) -> Element<'a, M> {
    let colors = *palette;
    let lines = items.into_iter().map(|item| -> Element<'a, M> {
        match item {
            ColumnMenuItem::Separator => container(
                container(Space::new())
                    .width(Length::Fill)
                    .height(spacing::BORDER_WIDTH)
                    .style(theme::menu_separator(palette)),
            )
            .padding([
                (MENU_SEPARATOR_HEIGHT - spacing::BORDER_WIDTH) / 2.0,
                spacing::CELL_PADDING_X,
            ])
            .height(MENU_SEPARATOR_HEIGHT)
            .into(),
            ColumnMenuItem::Entry {
                label,
                checked,
                note,
                on_press,
            } => {
                let enabled = on_press.is_some();
                let content = Row::new()
                    .push(
                        text(if checked { "✓" } else { "" })
                            .size(font_size::ROW)
                            .width(MENU_CHECK_WIDTH)
                            .wrapping(text::Wrapping::None),
                    )
                    .push(
                        text(label)
                            .size(font_size::ROW)
                            .width(Length::Fill)
                            .wrapping(text::Wrapping::None),
                    )
                    .push(
                        text(note.unwrap_or_default())
                            .size(font_size::STATUS)
                            .color(colors.menu_shortcut)
                            .wrapping(text::Wrapping::None),
                    )
                    .align_y(alignment::Vertical::Center);
                let line: Element<'a, M> = button(content)
                    .on_press_maybe(on_press)
                    .padding([0.0, spacing::CELL_PADDING_X])
                    .height(MENU_ITEM_HEIGHT)
                    .width(Length::Fill)
                    .style(move |_: &Theme, status| {
                        let hovered = enabled && matches!(status, button::Status::Hovered);
                        button::Style {
                            background: hovered.then_some(Background::Color(colors.cursor)),
                            text_color: if !enabled {
                                colors.menu_disabled
                            } else if hovered {
                                colors.cursor_text
                            } else {
                                colors.text
                            },
                            ..button::Style::default()
                        }
                    })
                    .into();
                line
            }
        }
    });
    let body = container(MenuColumn::with_children(lines))
        .padding(MENU_PADDING)
        .width(MENU_WIDTH)
        .style(theme::menu(palette));
    let backdrop = mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
        .on_press(on_close.clone())
        .on_right_press(on_close);
    stack![
        backdrop,
        pin(opaque(body))
            .x(origin.x)
            .y(origin.y)
            .width(Length::Fill)
            .height(Length::Fill)
    ]
    .into()
}

/// The column titles of a list that cannot be sorted or changed.
pub fn plain_column_header<'a, M: Clone + 'a>(
    palette: &Palette,
    layout: ColumnLayout<'a>,
) -> Element<'a, M> {
    column_header(palette, layout, None, false, None, None)
}

// A failing assertion in a test is the signal, so `unwrap` belongs here.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    const THREE: [Column; 3] = [Column::Icon, Column::Name, Column::Size];

    fn layout<'a>(columns: &'a [Column], widths: &'a [(Column, u16)]) -> ColumnLayout<'a> {
        ColumnLayout {
            columns,
            widths,
            ..ColumnLayout::standard(Language::English)
        }
    }

    #[test]
    fn a_dragged_width_replaces_the_standard_and_stays_within_bounds() {
        let widths = [
            (Column::Size, 120),
            (Column::Modified, 5),
            (Column::Type, 9000),
        ];
        let shown = layout(&THREE, &widths);
        assert_eq!(shown.width_of(Column::Size), 120.0);
        assert_eq!(shown.width_of(Column::Modified), MIN_COLUMN_WIDTH);
        assert_eq!(shown.width_of(Column::Type), MAX_COLUMN_WIDTH);
        assert_eq!(
            shown.width_of(Column::Owner),
            standard_column_width(Column::Owner)
        );
        assert_eq!(shown.width_of(Column::Icon), ICON_COLUMN_WIDTH);
    }

    #[test]
    fn the_name_takes_what_the_other_columns_leave() {
        let widths = [(Column::Size, 100)];
        let spans = column_spans(layout(&THREE, &widths), 600.0, false);
        let name = spans
            .iter()
            .find(|span| span.column == Column::Name)
            .unwrap();
        let used: f32 = spans.iter().map(|span| span.width).sum();
        assert_eq!(
            used + TAG_COLUMN_WIDTH + 2.0 * spacing::CELL_PADDING_X,
            600.0
        );
        assert_eq!(
            name.left,
            spacing::CELL_PADDING_X + TAG_COLUMN_WIDTH + ICON_COLUMN_WIDTH
        );
        let with_hermes = column_spans(layout(&THREE, &widths), 600.0, true);
        let narrower = with_hermes
            .iter()
            .find(|span| span.column == Column::Name)
            .unwrap();
        assert_eq!(narrower.width, name.width - HERMES_COLUMN_WIDTH);
    }

    #[test]
    fn the_columns_follow_each_other_in_the_order_of_the_layout() {
        let columns = [Column::Name, Column::Size, Column::Icon];
        let spans = column_spans(layout(&columns, &[]), 800.0, false);
        let order: Vec<Column> = spans.iter().map(|span| span.column).collect();
        assert_eq!(order, columns);
        for (first, second) in spans.iter().zip(spans.iter().skip(1)) {
            assert_eq!(first.left + first.width, second.left);
        }
    }

    #[test]
    fn a_column_this_system_cannot_show_is_left_out() {
        let columns = [Column::Name, Column::Owner];
        let shown: Vec<Column> = layout(&columns, &[]).shown().collect();
        assert_eq!(shown.contains(&Column::Owner), cfg!(unix));
    }
}
