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
    /// The entry the arrow keys stand on, counted over the entries only.
    pub selected: Option<usize>,
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
            selected: None,
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
                is_selected: false,
            }
        })
        .collect();
    let command = |label: Msg, checked: bool, message: PanelMessage| ColumnMenuItem::Entry {
        label: lang.text(label).to_owned(),
        checked,
        note: None,
        on_press: Some(message),
        is_selected: false,
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

/// What each entry sends when pressed, in order; `None` for one that is
/// disabled.
pub(crate) fn entry_messages(columns: &[Column], same_in_both: bool) -> Vec<Option<PanelMessage>> {
    items(columns, same_in_both, Language::default())
        .into_iter()
        .filter_map(|item| match item {
            ColumnMenuItem::Entry { on_press, .. } => Some(on_press),
            ColumnMenuItem::Separator => None,
        })
        .collect()
}

/// The entry `step` places on from `selected`, skipping disabled ones and
/// wrapping round. Nothing selected yet: down starts at the first, up at the
/// last.
pub(crate) fn step_selection(
    selected: Option<usize>,
    entries: &[Option<PanelMessage>],
    step: isize,
) -> Option<usize> {
    let count = isize::try_from(entries.len()).ok()?;
    let mut at = match selected.and_then(|index| isize::try_from(index).ok()) {
        Some(index) => index,
        None if step > 0 => -1,
        None => 0,
    };
    for _ in 0..count {
        at = (at + step.signum()).rem_euclid(count);
        let index = usize::try_from(at).ok()?;
        if entries.get(index).is_some_and(Option::is_some) {
            return Some(index);
        }
    }
    selected
}

/// The menu as an overlay for the whole window; the app lays it over the
/// panels while `Panels::header_menu` is set.
pub fn header_menu_view<'a>(
    menu: &HeaderMenu,
    columns: ColumnSettings<'_>,
    lang: Language,
    palette: &Palette,
) -> iced::Element<'a, PanelMessage> {
    let mut items = items(columns.of(menu.side), columns.same_in_both, lang);
    let mut entry = 0;
    for item in &mut items {
        if let ColumnMenuItem::Entry { is_selected, .. } = item {
            *is_selected = menu.selected == Some(entry);
            entry += 1;
        }
    }
    column_menu(items, menu.origin, PanelMessage::HeaderMenuClose, palette)
}

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
                    ..
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
    fn the_arrows_skip_the_name_and_wrap_round() {
        let entries = entry_messages(&Column::DEFAULT, true);
        let first = entries.iter().position(Option::is_some).unwrap();
        let last = entries.len() - 1;
        assert_eq!(step_selection(None, &entries, 1), Some(first));
        assert_eq!(step_selection(None, &entries, -1), Some(last));
        assert_eq!(step_selection(Some(last), &entries, 1), Some(first));
        assert_eq!(step_selection(Some(first), &entries, -1), Some(last));
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
