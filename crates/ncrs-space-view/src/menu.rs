//! The entries of the menu of a row: the view draws them, the keys walk them.

use ncrs_i18n::Msg;

use crate::message::MenuEntry;
use crate::shortcuts::Shortcut;
use crate::state::State;

pub struct MenuItem {
    pub label: Msg,
    pub key: Shortcut,
    pub entry: MenuEntry,
    pub enabled: bool,
    /// A line is drawn above: the entry starts a group.
    pub starts_group: bool,
    /// It destroys something: drawn in the error color.
    pub is_danger: bool,
}

/// What the menu of the row under the cursor offers.
pub fn menu_items(state: &State) -> Vec<MenuItem> {
    let deletable = state
        .menu_path()
        .and_then(|path| state.row_by_path(path))
        .is_some_and(|row| row.is_deletable());
    vec![
        MenuItem {
            label: Msg::SpaceShowInPanel,
            key: Shortcut::ShowInPanel,
            entry: MenuEntry::ShowInPanel,
            enabled: true,
            starts_group: false,
            is_danger: false,
        },
        MenuItem {
            label: Msg::SpaceAsk,
            key: Shortcut::AskHermes,
            entry: MenuEntry::AskHermes,
            enabled: true,
            starts_group: true,
            is_danger: false,
        },
        MenuItem {
            label: Msg::SpaceDeleteOnlyThis,
            key: Shortcut::DeleteOnlyThis,
            entry: MenuEntry::DeleteOnlyThis,
            enabled: deletable,
            starts_group: true,
            is_danger: true,
        },
    ]
}

/// The next entry that can be picked from `from` in `step` direction, round the end;
/// `from` itself when no other can.
pub fn next_enabled(items: &[MenuItem], from: usize, step: isize) -> usize {
    let count = items.len();
    (1..=count)
        .map(|distance| {
            (from as isize + step * distance as isize).rem_euclid(count.max(1) as isize) as usize
        })
        .find(|&index| items.get(index).is_some_and(|item| item.enabled))
        .unwrap_or(from)
}
