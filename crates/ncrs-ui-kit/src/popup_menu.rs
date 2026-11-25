//! A small menu laid over the window at a point: entries with their key at
//! the right. Looks like the context menu of the file panels. The parts it is
//! built from are public so a menu with other entries looks the same.

use iced::widget::text::Wrapping;
use iced::widget::{
    button, column, container, mouse_area, opaque, pin, row, stack, text, Column, Space,
};
use iced::{alignment, Color, Element, Length, Point, Size};

use crate::layout::{
    MENU_ITEM_HEIGHT, MENU_PADDING, MENU_SEPARATOR_HEIGHT, MENU_SUB_WIDTH, MENU_WIDTH,
};
use crate::palette::Palette;
use crate::theme::{self, font_size, spacing};

#[derive(Debug, Clone)]
pub struct PopupEntry<M> {
    pub label: String,
    pub key: Option<String>,
    pub is_selected: bool,
    /// `None` draws the entry as disabled.
    pub on_press: Option<M>,
    pub on_hover: Option<M>,
}

/// The height of a menu with `entries` entries, for keeping it in the window.
pub fn popup_menu_height(entries: usize) -> f32 {
    entries as f32 * MENU_ITEM_HEIGHT + 2.0 * MENU_PADDING
}

pub const POPUP_MENU_WIDTH: f32 = MENU_WIDTH;

/// The colors of an entry's label and of what stands beside it (its key or a
/// note), by whether it can be pressed and whether the cursor is on it.
pub fn menu_entry_colors(palette: &Palette, enabled: bool, is_selected: bool) -> (Color, Color) {
    match (enabled, is_selected) {
        (false, _) => (palette.menu_disabled, palette.menu_disabled),
        (true, true) => (palette.cursor_text, palette.menu_shortcut_on_cursor),
        (true, false) => (palette.text, palette.menu_shortcut),
    }
}

/// One pressable row of a menu around `content`. The pointer over an enabled
/// row sends `on_hover`.
pub fn menu_item<'a, M: Clone + 'a>(
    content: impl Into<Element<'a, M>>,
    is_selected: bool,
    on_press: Option<M>,
    on_hover: Option<M>,
    palette: &Palette,
) -> Element<'a, M> {
    let enabled = on_press.is_some();
    let item = button(content)
        .on_press_maybe(on_press)
        .padding([0.0, spacing::CELL_PADDING_X])
        .height(MENU_ITEM_HEIGHT)
        .width(Length::Fill)
        .style(theme::menu_item(palette, is_selected));
    match (enabled, on_hover) {
        (true, Some(hover)) => Element::from(mouse_area(item).on_enter(hover)),
        _ => item.into(),
    }
}

/// The line between two groups of a menu.
pub fn menu_separator<'a, M: 'a>(palette: &Palette) -> Element<'a, M> {
    container(
        container(column![])
            .width(Length::Fill)
            .height(spacing::BORDER_WIDTH)
            .style(theme::menu_separator(palette)),
    )
    .padding([
        (MENU_SEPARATOR_HEIGHT - spacing::BORDER_WIDTH) / 2.0,
        spacing::CELL_PADDING_X,
    ])
    .height(MENU_SEPARATOR_HEIGHT)
    .into()
}

/// The rows of a menu in its frame, `width` wide, laid over the window with
/// its corner at `origin`.
pub fn menu_box<'a, M: 'a>(
    items: impl IntoIterator<Item = Element<'a, M>>,
    width: f32,
    origin: Point,
    palette: &Palette,
) -> Element<'a, M> {
    let body = container(Column::with_children(items))
        .padding(MENU_PADDING)
        .width(width)
        .style(theme::menu(palette));
    // `opaque` so a click on the menu's own padding is not taken for a click
    // beside it.
    pin(opaque(body))
        .x(origin.x)
        .y(origin.y)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// What a right press beside an open menu does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightPress {
    /// It closes the menu like a left press.
    Dismiss,
    /// It reaches what is under the menu, so another row's menu can replace
    /// this one.
    PassThrough,
}

/// `menu` over a layer that catches the presses beside it. A left press sends
/// `on_dismiss` (`None`: it passes through too).
pub fn popup_layer<'a, M: Clone + 'a>(
    menu: Element<'a, M>,
    on_dismiss: Option<M>,
    right_press: RightPress,
) -> Element<'a, M> {
    let mut catcher = mouse_area(Space::new().width(Length::Fill).height(Length::Fill));
    if let Some(message) = on_dismiss.clone() {
        catcher = catcher.on_press(message);
    }
    if let (RightPress::Dismiss, Some(message)) = (right_press, on_dismiss) {
        catcher = catcher.on_right_press(message);
    }
    stack![catcher, menu].into()
}

pub fn popup_menu<'a, M: Clone + 'a>(
    entries: Vec<PopupEntry<M>>,
    origin: Point,
    palette: &Palette,
) -> Element<'a, M> {
    let items = entries.into_iter().map(|entry| {
        let (label_color, key_color) =
            menu_entry_colors(palette, entry.on_press.is_some(), entry.is_selected);
        let content = row![
            text(entry.label)
                .size(font_size::ROW)
                .color(label_color)
                .width(Length::Fill)
                .wrapping(Wrapping::None),
            text(entry.key.unwrap_or_default())
                .size(font_size::STATUS)
                .color(key_color)
                .wrapping(Wrapping::None),
        ]
        .align_y(alignment::Vertical::Center);
        menu_item(
            content,
            entry.is_selected,
            entry.on_press,
            entry.on_hover,
            palette,
        )
    });
    menu_box(items, POPUP_MENU_WIDTH, origin, palette)
}

/// A line of a menu that has separators between its groups.
#[derive(Debug, Clone)]
pub enum MenuRow<M> {
    Entry(PopupEntry<M>),
    Separator,
    /// An entry with a menu of its own, open while the entry is the selected
    /// one. Its `key` is not shown: the arrow takes the place.
    Submenu(PopupEntry<M>, Vec<MenuRow<M>>),
    /// An entry that destroys something, in the error color.
    Danger(PopupEntry<M>),
}

const SUBMENU_ARROW: &str = "\u{25b8}";

/// A row of a menu: the label, the key (or `trail`) at the right end if there
/// is one.
fn entry_item<'a, M: Clone + 'a>(
    entry: PopupEntry<M>,
    trail: Option<&str>,
    danger: bool,
    palette: &Palette,
) -> Element<'a, M> {
    let (mut label_color, key_color) =
        menu_entry_colors(palette, entry.on_press.is_some(), entry.is_selected);
    if danger && entry.on_press.is_some() && !entry.is_selected {
        label_color = palette.error;
    }
    let key_text = trail.map(str::to_owned).or(entry.key).unwrap_or_default();
    let content = row![
        text(entry.label)
            .size(font_size::ROW)
            .color(label_color)
            .width(Length::Fill)
            .wrapping(Wrapping::None),
        text(key_text)
            .size(font_size::STATUS)
            .color(key_color)
            .wrapping(Wrapping::None),
    ]
    .align_y(alignment::Vertical::Center);
    menu_item(
        content,
        entry.is_selected,
        entry.on_press,
        entry.on_hover,
        palette,
    )
}

/// The height of a menu with `entries` entries and `separators` separators.
pub fn popup_rows_height(entries: usize, separators: usize) -> f32 {
    popup_menu_height(entries) + separators as f32 * MENU_SEPARATOR_HEIGHT
}

fn row_height<M>(line: &MenuRow<M>) -> f32 {
    match line {
        MenuRow::Separator => MENU_SEPARATOR_HEIGHT,
        MenuRow::Entry(_) | MenuRow::Submenu(..) | MenuRow::Danger(_) => MENU_ITEM_HEIGHT,
    }
}

/// The height of a menu with these rows, frame included.
pub fn menu_rows_height<M>(rows: &[MenuRow<M>]) -> f32 {
    2.0 * MENU_PADDING + rows.iter().map(row_height).sum::<f32>()
}

/// Where the corner of a submenu goes. It opens at the right of its parent
/// menu with its first row level with the parent entry, which is
/// `row_offset` below the top of the parent menu's rows; it folds to the left
/// where `window` has no room at the right, and up where it has none below.
pub fn submenu_origin(
    parent_origin: Point,
    parent_width: f32,
    row_offset: f32,
    submenu: Size,
    window: Size,
) -> Point {
    let right = parent_origin.x + parent_width;
    let x = if right + submenu.width > window.width {
        (parent_origin.x - submenu.width).max(0.0)
    } else {
        right
    };
    let top = parent_origin.y + row_offset;
    let y = top.min(window.height - submenu.height).max(0.0);
    Point::new(x, y)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmenuArrow {
    Right,
    Left,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmenuStep {
    Open,
    Close,
    /// The arrow does nothing to a submenu.
    Ignore,
}

/// What an arrow key does: Right opens the submenu of the entry the cursor is
/// on, Left closes the open one.
pub fn submenu_step(arrow: SubmenuArrow, cursor_on_submenu: bool, is_open: bool) -> SubmenuStep {
    match arrow {
        SubmenuArrow::Right if cursor_on_submenu && !is_open => SubmenuStep::Open,
        SubmenuArrow::Left if is_open => SubmenuStep::Close,
        SubmenuArrow::Right | SubmenuArrow::Left => SubmenuStep::Ignore,
    }
}

fn push_menu_layers<'a, M: Clone + 'a>(
    rows: Vec<MenuRow<M>>,
    width: f32,
    origin: Point,
    window: Size,
    palette: &Palette,
    layers: &mut Vec<Element<'a, M>>,
) {
    let mut items = Vec::new();
    let mut opened = Vec::new();
    let mut offset = 0.0;
    for line in rows {
        let height = row_height(&line);
        match line {
            MenuRow::Entry(entry) => items.push(entry_item(entry, None, false, palette)),
            MenuRow::Danger(entry) => items.push(entry_item(entry, None, true, palette)),
            MenuRow::Separator => items.push(menu_separator(palette)),
            MenuRow::Submenu(entry, children) => {
                if entry.is_selected {
                    opened.push((offset, children));
                }
                items.push(entry_item(entry, Some(SUBMENU_ARROW), false, palette));
            }
        }
        offset += height;
    }
    layers.push(menu_box(items, width, origin, palette));
    for (row_offset, children) in opened {
        let size = Size::new(MENU_SUB_WIDTH, menu_rows_height(&children));
        let sub_origin = submenu_origin(origin, width, row_offset, size, window);
        push_menu_layers(
            children,
            MENU_SUB_WIDTH,
            sub_origin,
            window,
            palette,
            layers,
        );
    }
}

/// A menu of `width` with groups: like [`popup_menu`], with a line between them.
/// A submenu is drawn as an entry with an arrow, without its menu.
pub fn popup_menu_rows<'a, M: Clone + 'a>(
    rows: Vec<MenuRow<M>>,
    width: f32,
    origin: Point,
    palette: &Palette,
) -> Element<'a, M> {
    popup_menu_rows_in(rows, width, origin, Size::new(f32::MAX, f32::MAX), palette)
}

/// As [`popup_menu_rows`], with the menus of the open submenus laid beside
/// it inside `window`.
pub fn popup_menu_rows_in<'a, M: Clone + 'a>(
    rows: Vec<MenuRow<M>>,
    width: f32,
    origin: Point,
    window: Size,
    palette: &Palette,
) -> Element<'a, M> {
    let mut layers = Vec::new();
    push_menu_layers(rows, width, origin, window, palette, &mut layers);
    stack(layers).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Size = Size::new(1000.0, 700.0);
    const SUB: Size = Size::new(MENU_SUB_WIDTH, 120.0);

    #[test]
    fn a_submenu_opens_at_the_right_of_its_entry() {
        let origin = submenu_origin(Point::new(100.0, 50.0), 300.0, 48.0, SUB, WINDOW);
        assert_eq!(origin, Point::new(400.0, 98.0));
    }

    #[test]
    fn a_submenu_folds_to_the_left_at_the_right_edge_of_the_window() {
        let origin = submenu_origin(Point::new(800.0, 50.0), 300.0, 0.0, SUB, WINDOW);
        assert_eq!(origin.x, 800.0 - MENU_SUB_WIDTH);
        assert_eq!(origin.y, 50.0);
    }

    #[test]
    fn a_submenu_stays_above_the_bottom_edge() {
        let origin = submenu_origin(Point::new(100.0, 600.0), 300.0, 100.0, SUB, WINDOW);
        assert_eq!(origin.y, WINDOW.height - SUB.height);
    }

    #[test]
    fn right_opens_the_submenu_under_the_cursor_and_left_closes_it() {
        use SubmenuArrow::{Left, Right};
        assert_eq!(submenu_step(Right, true, false), SubmenuStep::Open);
        assert_eq!(submenu_step(Right, false, false), SubmenuStep::Ignore);
        assert_eq!(submenu_step(Right, true, true), SubmenuStep::Ignore);
        assert_eq!(submenu_step(Left, true, true), SubmenuStep::Close);
        assert_eq!(submenu_step(Left, true, false), SubmenuStep::Ignore);
    }

    #[test]
    fn the_height_of_rows_counts_separators_smaller() {
        let entry = || PopupEntry::<()> {
            label: String::new(),
            key: None,
            is_selected: false,
            on_press: None,
            on_hover: None,
        };
        let rows = vec![
            MenuRow::Entry(entry()),
            MenuRow::Separator,
            MenuRow::Danger(entry()),
        ];
        assert_eq!(menu_rows_height(&rows), popup_rows_height(2, 1));
    }
}
