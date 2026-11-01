//! The context menu of a panel row: entries with their keys on the right,
//! groups divided by lines, laid over the window at the position the menu
//! was given.

use iced::widget::{
    button, column, container, mouse_area, opaque, pin, row, stack, text, Column, Space,
};
use iced::{alignment, Element, Length};

use super::kit::{self, Icon};
use super::layout::{
    MENU_ITEM_HEIGHT, MENU_PADDING, MENU_SEPARATOR_HEIGHT, MENU_SUB_WIDTH, MENU_WIDTH,
};
use super::theme::{self, font_size, spacing};
use crate::context_menu::{ContextMenu, MenuEntry, SubEntry};
use crate::i18n::{Language, Msg};
use crate::keymap::Keymap;
use crate::keymap::KeymapMessages as _;
use crate::messages::Message;
use crate::palette::Palette;

pub fn view<'a>(
    menu: &'a ContextMenu,
    lang: Language,
    keymap: &Keymap,
    palette: &Palette,
) -> Element<'a, Message> {
    let entries = menu
        .entries()
        .iter()
        .enumerate()
        .map(|(index, entry)| match entry {
            MenuEntry::Separator => separator(palette),
            MenuEntry::Hermes => {
                let is_selected = index == menu.selected();
                let label_color = if is_selected {
                    palette.cursor_text
                } else {
                    palette.text
                };
                let content = row![
                    text(lang.text(Msg::ColumnHermes))
                        .size(font_size::ROW)
                        .color(label_color)
                        .width(Length::Fill)
                        .wrapping(text::Wrapping::None),
                    kit::icon(Icon::ChevronRight, label_color),
                ]
                .align_y(alignment::Vertical::Center);
                let item = button(content)
                    .on_press(Message::ContextMenuClick(index))
                    .padding([0.0, spacing::CELL_PADDING_X])
                    .height(MENU_ITEM_HEIGHT)
                    .width(Length::Fill)
                    .style(theme::menu_item(palette, is_selected));
                mouse_area(item)
                    .on_enter(Message::ContextMenuHover(index))
                    .into()
            }
            MenuEntry::Item { action, enabled } => {
                let is_selected = index == menu.selected();
                let label_color = match (*enabled, is_selected) {
                    (false, _) => palette.menu_disabled,
                    (true, true) => palette.cursor_text,
                    (true, false) => palette.text,
                };
                let shortcut_color = match (*enabled, is_selected) {
                    (false, _) => palette.menu_disabled,
                    (true, true) => palette.menu_shortcut_on_cursor,
                    (true, false) => palette.menu_shortcut,
                };
                let shortcut = keymap.shortcut_label(&action.message()).unwrap_or_default();
                let content = row![
                    text(lang.text(action.label()))
                        .size(font_size::ROW)
                        .color(label_color)
                        .width(Length::Fill)
                        .wrapping(text::Wrapping::None),
                    text(shortcut)
                        .size(font_size::STATUS)
                        .color(shortcut_color)
                        .wrapping(text::Wrapping::None),
                ]
                .align_y(alignment::Vertical::Center);
                let item = button(content)
                    .on_press_maybe(enabled.then_some(Message::ContextMenuClick(index)))
                    .padding([0.0, spacing::CELL_PADDING_X])
                    .height(MENU_ITEM_HEIGHT)
                    .width(Length::Fill)
                    .style(theme::menu_item(palette, is_selected));
                if *enabled {
                    mouse_area(item)
                        .on_enter(Message::ContextMenuHover(index))
                        .into()
                } else {
                    item.into()
                }
            }
        });

    let body = container(Column::with_children(entries))
        .padding(MENU_PADDING)
        .width(MENU_WIDTH)
        .style(theme::menu(palette));

    // `opaque` so a click on the menu's own padding is not taken for a click
    // beside it.
    let main = pin(opaque(body))
        .x(menu.origin().x)
        .y(menu.origin().y)
        .width(Length::Fill)
        .height(Length::Fill);
    match menu.submenu_origin() {
        Some(origin) => stack![
            main,
            pin(opaque(submenu(menu, lang, keymap, palette)))
                .x(origin.x)
                .y(origin.y)
                .width(Length::Fill)
                .height(Length::Fill)
        ]
        .into(),
        None => main.into(),
    }
}

fn submenu<'a>(
    menu: &'a ContextMenu,
    lang: Language,
    keymap: &Keymap,
    palette: &Palette,
) -> Element<'a, Message> {
    let entries = menu
        .sub_entries()
        .iter()
        .enumerate()
        .map(|(index, entry)| match entry {
            SubEntry::Separator => separator(palette),
            SubEntry::Item { action, enabled } => {
                let is_selected = index == menu.sub_selected();
                let (label_color, note_color) = match (*enabled, is_selected) {
                    (false, _) => (palette.menu_disabled, palette.menu_disabled),
                    (true, true) => (palette.cursor_text, palette.menu_shortcut_on_cursor),
                    (true, false) => (palette.text, palette.menu_shortcut),
                };
                let shortcut = keymap.shortcut_label(&action.message());
                let content = row![text(lang.text(action.label()))
                    .size(font_size::ROW)
                    .color(label_color)
                    .wrapping(text::Wrapping::None)]
                .extend(action.note().map(|note| {
                    text(lang.text(note))
                        .size(font_size::STATUS)
                        .color(note_color)
                        .wrapping(text::Wrapping::None)
                        .into()
                }))
                .push(Space::new().width(Length::Fill))
                .extend(
                    shortcut
                        .map(|keys| kit::key_cap_in_row(&keys, is_selected && *enabled, palette)),
                )
                .spacing(spacing::S)
                .align_y(alignment::Vertical::Center);
                let item = button(content)
                    .on_press_maybe(enabled.then_some(Message::ContextMenuSubClick(index)))
                    .padding([0.0, spacing::CELL_PADDING_X])
                    .height(MENU_ITEM_HEIGHT)
                    .width(Length::Fill)
                    .style(theme::menu_item(palette, is_selected));
                if *enabled {
                    mouse_area(item)
                        .on_enter(Message::ContextMenuSubHover(index))
                        .into()
                } else {
                    item.into()
                }
            }
        });
    container(Column::with_children(entries))
        .padding(MENU_PADDING)
        .width(MENU_SUB_WIDTH)
        .style(theme::menu(palette))
        .into()
}

fn separator<'a>(palette: &Palette) -> Element<'a, Message> {
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
