//! The context menu of a panel row: entries with their keys on the right,
//! groups divided by lines, laid over the window at the position the menu
//! was given.

use iced::widget::{button, column, container, mouse_area, opaque, pin, row, text, Column};
use iced::{alignment, Element, Length};

use super::layout::{MENU_ITEM_HEIGHT, MENU_PADDING, MENU_SEPARATOR_HEIGHT, MENU_WIDTH};
use super::theme::{self, colors, font_size, spacing};
use crate::context_menu::{ContextMenu, MenuEntry};
use crate::i18n::Language;
use crate::keymap;
use crate::messages::Message;

pub fn view(menu: &ContextMenu, lang: Language) -> Element<'_, Message> {
    let entries = menu
        .entries()
        .iter()
        .enumerate()
        .map(|(index, entry)| match entry {
            MenuEntry::Separator => separator(),
            MenuEntry::Item { action, enabled } => {
                let is_selected = index == menu.selected();
                let label_color = match (*enabled, is_selected) {
                    (false, _) => colors::DISABLED_TEXT,
                    (true, true) => colors::SELECTED_TEXT,
                    (true, false) => colors::TEXT,
                };
                let shortcut_color = match (*enabled, is_selected) {
                    (false, _) => colors::DISABLED_TEXT,
                    (true, true) => colors::SELECTED_TEXT,
                    (true, false) => colors::DIM_TEXT,
                };
                let shortcut = keymap::shortcut_label(&action.message()).unwrap_or_default();
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
                    .style(theme::menu_item(is_selected));
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
        .style(theme::dialog);

    // `opaque` so a click on the menu's own padding is not taken for a click
    // beside it.
    pin(opaque(body))
        .x(menu.origin().x)
        .y(menu.origin().y)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn separator<'a>() -> Element<'a, Message> {
    container(
        container(column![])
            .width(Length::Fill)
            .height(spacing::BORDER_WIDTH)
            .style(theme::menu_separator),
    )
    .padding([
        (MENU_SEPARATOR_HEIGHT - spacing::BORDER_WIDTH) / 2.0,
        spacing::CELL_PADDING_X,
    ])
    .height(MENU_SEPARATOR_HEIGHT)
    .into()
}
