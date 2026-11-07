//! The menu a right click on a column title opens: which columns the panel
//! shows, and the way to the column settings.

use iced::{Point, Size};
use ncrs_core::{Column, PanelSide};
use ncrs_i18n::{Language, Msg};
use ncrs_ui_kit::column_header::{column_menu, column_menu_height, column_message, ColumnMenuItem};
use ncrs_ui_kit::layout::{place, MENU_WIDTH};
use ncrs_ui_kit::palette::Palette;

use crate::ctx::ColumnSettings;
use crate::message::PanelMessage;

/// The menu while it is up. It stays up while columns are ticked.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeaderMenu {
    pub side: PanelSide,
    /// Top left corner, already kept inside the window.
    pub origin: Point,
}

/// The columns the menu lists: every one this system can show.
fn listed_columns() -> impl Iterator<Item = Column> {
    Column::ALL
        .into_iter()
        .filter(|column| column.on_this_system())
}

/// Entries and separators of the menu: the columns, then the two settings,
/// then the two commands.
fn counts() -> (usize, usize) {
    (listed_columns().count() + 3, 2)
}

impl HeaderMenu {
    /// A menu opened by a right click at `pointer` in a window of `window`.
    pub fn at(side: PanelSide, pointer: Point, window: Size) -> Self {
        let (entries, separators) = counts();
        Self {
            side,
            origin: Point::new(
                place(pointer.x, MENU_WIDTH, window.width),
                place(
                    pointer.y,
                    column_menu_height(entries, separators),
                    window.height,
                ),
            ),
        }
    }
}

fn items(
    columns: &[Column],
    same_in_both: bool,
    lang: Language,
) -> Vec<ColumnMenuItem<PanelMessage>> {
    let mut items: Vec<ColumnMenuItem<PanelMessage>> = listed_columns()
        .map(|column| {
            let is_name = column == Column::Name;
            ColumnMenuItem::Entry {
                label: lang.text(column_message(column)).to_owned(),
                checked: columns.contains(&column),
                note: is_name.then(|| lang.text(Msg::ColumnMenuAlways).to_owned()),
                on_press: (!is_name).then_some(PanelMessage::HeaderMenuToggleColumn(column)),
            }
        })
        .collect();
    let command = |label: Msg, checked: bool, message: PanelMessage| ColumnMenuItem::Entry {
        label: lang.text(label).to_owned(),
        checked,
        note: None,
        on_press: Some(message),
    };
    items.push(ColumnMenuItem::Separator);
    items.push(command(
        Msg::ColumnMenuSameInBoth,
        same_in_both,
        PanelMessage::HeaderMenuSameInBoth,
    ));
    items.push(ColumnMenuItem::Separator);
    items.push(command(
        Msg::ColumnMenuConfigure,
        false,
        PanelMessage::HeaderMenuConfigure,
    ));
    items.push(command(
        Msg::ColumnMenuRestore,
        false,
        PanelMessage::HeaderMenuRestore,
    ));
    items
}

/// The menu as an overlay for the whole window; the app lays it over the
/// panels while `Panels::header_menu` is set.
pub fn header_menu_view<'a>(
    menu: &HeaderMenu,
    columns: ColumnSettings<'_>,
    lang: Language,
    palette: &Palette,
) -> iced::Element<'a, PanelMessage> {
    column_menu(
        items(columns.of(menu.side), columns.same_in_both, lang),
        menu.origin,
        PanelMessage::HeaderMenuClose,
        palette,
    )
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

    #[test]
    fn the_name_is_listed_ticked_and_cannot_be_switched_off() {
        let items = items(&Column::DEFAULT, true, Language::English);
        let name = items
            .iter()
            .find_map(|item| match item {
                ColumnMenuItem::Entry {
                    label,
                    checked,
                    note,
                    on_press,
                } if label == "Name" => Some((*checked, note.clone(), on_press.clone())),
                _ => None,
            })
            .unwrap();
        assert_eq!(name, (true, Some("always".to_owned()), None));
    }

    #[test]
    fn the_menu_lists_every_column_this_system_shows_and_four_more_lines() {
        let items = items(&Column::DEFAULT, false, Language::English);
        let (entries, separators) = counts();
        let separator_count = items
            .iter()
            .filter(|item| matches!(item, ColumnMenuItem::Separator))
            .count();
        assert_eq!(items.len(), entries + separators);
        assert_eq!(separator_count, separators);
        assert!(!items.iter().any(|item| matches!(
            item,
            ColumnMenuItem::Entry { label, .. } if label == "Owner" && !cfg!(unix)
        )));
    }

    #[test]
    fn a_menu_at_the_edge_of_the_window_is_pulled_inside() {
        let window = Size::new(1200.0, 760.0);
        let menu = HeaderMenu::at(PanelSide::Right, Point::new(1190.0, 750.0), window);
        let (entries, separators) = counts();
        assert!(menu.origin.x + MENU_WIDTH <= window.width);
        assert!(menu.origin.y + column_menu_height(entries, separators) <= window.height);
    }
}
