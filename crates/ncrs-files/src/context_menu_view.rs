//! The context menu of a panel row: entries with their keys on the right,
//! groups divided by lines, laid over the window at the position the menu
//! was given.

use iced::widget::text::Wrapping;
use iced::widget::{row, stack, text, Space};
use iced::{alignment, Element, Length};

use ncrs_i18n::{Language, Msg};
use ncrs_ui_kit::kit::{self, Icon};
use ncrs_ui_kit::layout::{MENU_SUB_WIDTH, MENU_WIDTH};
use ncrs_ui_kit::palette::Palette;
use ncrs_ui_kit::popup_menu::{menu_box, menu_entry_colors, menu_item, menu_separator};
use ncrs_ui_kit::theme::{font_size, spacing};

use crate::context_menu::{ContextAction, ContextMenu, HermesAction, MenuEntry, SubEntry};

/// What a click or the pointer over an entry sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextMenuMessage {
    Click(usize),
    Hover(usize),
    SubClick(usize),
    SubHover(usize),
}

/// The keys the entries show, spelled for this platform and keymap.
pub struct EntryKeys<'a> {
    pub context: &'a dyn Fn(ContextAction) -> Option<String>,
    pub hermes: &'a dyn Fn(HermesAction) -> Option<String>,
}

impl std::fmt::Debug for EntryKeys<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EntryKeys").finish_non_exhaustive()
    }
}

pub fn view<'a>(
    menu: &'a ContextMenu,
    lang: Language,
    keys: &EntryKeys<'_>,
    palette: &Palette,
) -> Element<'a, ContextMenuMessage> {
    let entries = menu
        .entries()
        .iter()
        .enumerate()
        .map(|(index, entry)| match entry {
            MenuEntry::Separator => menu_separator(palette),
            MenuEntry::Hermes => {
                let is_selected = index == menu.selected();
                let (label_color, _) = menu_entry_colors(palette, true, is_selected);
                let content = row![
                    text(lang.text(Msg::ColumnHermes))
                        .size(font_size::ROW)
                        .color(label_color)
                        .width(Length::Fill)
                        .wrapping(Wrapping::None),
                    kit::icon(Icon::ChevronRight, label_color),
                ]
                .align_y(alignment::Vertical::Center);
                menu_item(
                    content,
                    is_selected,
                    Some(ContextMenuMessage::Click(index)),
                    Some(ContextMenuMessage::Hover(index)),
                    palette,
                )
            }
            MenuEntry::Item { action, enabled } => {
                let is_selected = index == menu.selected();
                let (label_color, shortcut_color) =
                    menu_entry_colors(palette, *enabled, is_selected);
                let shortcut = (keys.context)(*action).unwrap_or_default();
                let content = row![
                    text(lang.text(action.label()))
                        .size(font_size::ROW)
                        .color(label_color)
                        .width(Length::Fill)
                        .wrapping(Wrapping::None),
                    text(shortcut)
                        .size(font_size::STATUS)
                        .color(shortcut_color)
                        .wrapping(Wrapping::None),
                ]
                .align_y(alignment::Vertical::Center);
                menu_item(
                    content,
                    is_selected,
                    enabled.then_some(ContextMenuMessage::Click(index)),
                    Some(ContextMenuMessage::Hover(index)),
                    palette,
                )
            }
        });

    let main = menu_box(entries, MENU_WIDTH, menu.origin(), palette);
    match menu.submenu_origin() {
        Some(origin) => stack![main, submenu(menu, lang, keys, origin, palette)].into(),
        None => main,
    }
}

fn submenu<'a>(
    menu: &'a ContextMenu,
    lang: Language,
    keys: &EntryKeys<'_>,
    origin: iced::Point,
    palette: &Palette,
) -> Element<'a, ContextMenuMessage> {
    let entries = menu
        .sub_entries()
        .iter()
        .enumerate()
        .map(|(index, entry)| match entry {
            SubEntry::Separator => menu_separator(palette),
            SubEntry::Item { action, enabled } => {
                let is_selected = index == menu.sub_selected();
                let (label_color, note_color) = menu_entry_colors(palette, *enabled, is_selected);
                let shortcut = (keys.hermes)(*action);
                let content = row![text(lang.text(action.label()))
                    .size(font_size::ROW)
                    .color(label_color)
                    .wrapping(Wrapping::None)]
                .extend(action.note().map(|note| {
                    text(lang.text(note))
                        .size(font_size::STATUS)
                        .color(note_color)
                        .wrapping(Wrapping::None)
                        .into()
                }))
                .push(Space::new().width(Length::Fill))
                .extend(
                    shortcut
                        .map(|caps| kit::key_cap_in_row(&caps, is_selected && *enabled, palette)),
                )
                .spacing(spacing::XS)
                .align_y(alignment::Vertical::Center);
                menu_item(
                    content,
                    is_selected,
                    enabled.then_some(ContextMenuMessage::SubClick(index)),
                    Some(ContextMenuMessage::SubHover(index)),
                    palette,
                )
            }
        });
    menu_box(entries, MENU_SUB_WIDTH, origin, palette)
}
