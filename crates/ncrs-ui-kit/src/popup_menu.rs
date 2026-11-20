//! A small menu laid over the window at a point: entries with their key at
//! the right. Looks like the context menu of the file panels. The parts it is
//! built from are public so a menu with other entries looks the same.

use iced::widget::text::Wrapping;
use iced::widget::{
    button, column, container, mouse_area, opaque, pin, row, stack, text, Column, Space,
};
use iced::{alignment, Color, Element, Length, Point};

use crate::layout::{MENU_ITEM_HEIGHT, MENU_PADDING, MENU_SEPARATOR_HEIGHT, MENU_WIDTH};
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
