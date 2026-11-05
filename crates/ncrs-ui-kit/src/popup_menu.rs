//! A small menu laid over the window at a point: entries with their key at
//! the right. Looks like the context menu of the file panels.

use iced::widget::text::Wrapping;
use iced::widget::{button, container, mouse_area, opaque, pin, row, text, Column};
use iced::{alignment, Element, Length, Point};

use crate::layout::{MENU_ITEM_HEIGHT, MENU_PADDING, MENU_WIDTH};
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

pub fn popup_menu<'a, M: Clone + 'a>(
    entries: Vec<PopupEntry<M>>,
    origin: Point,
    palette: &Palette,
) -> Element<'a, M> {
    let items = entries.into_iter().map(|entry| {
        let enabled = entry.on_press.is_some();
        let (label_color, key_color) = match (enabled, entry.is_selected) {
            (false, _) => (palette.menu_disabled, palette.menu_disabled),
            (true, true) => (palette.cursor_text, palette.menu_shortcut_on_cursor),
            (true, false) => (palette.text, palette.menu_shortcut),
        };
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
        let item = button(content)
            .on_press_maybe(entry.on_press)
            .padding([0.0, spacing::CELL_PADDING_X])
            .height(MENU_ITEM_HEIGHT)
            .width(Length::Fill)
            .style(theme::menu_item(palette, entry.is_selected));
        match (enabled, entry.on_hover) {
            (true, Some(hover)) => Element::from(mouse_area(item).on_enter(hover)),
            _ => item.into(),
        }
    });
    let body = container(Column::with_children(items))
        .padding(MENU_PADDING)
        .width(POPUP_MENU_WIDTH)
        .style(theme::menu(palette));
    pin(opaque(body))
        .x(origin.x)
        .y(origin.y)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
