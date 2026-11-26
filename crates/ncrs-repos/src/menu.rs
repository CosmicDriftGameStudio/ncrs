//! The entries of the menu of a row: the view draws them, the keys walk them.

use ncrs_i18n::Msg;

use crate::bulk::BulkAction;
use crate::message::MenuEntry;
use crate::shortcuts::Shortcut;
use crate::state::State;

/// Where the key shown at the right of an entry comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuKey {
    /// The key of the tab for this action.
    Own(Shortcut),
    /// The app's key that opens the thing under the cursor, Enter by default.
    AppOpen,
    AppTerminal,
    AppCopyPath,
}

pub struct MenuItem {
    pub label: Msg,
    pub key: MenuKey,
    pub entry: MenuEntry,
    pub enabled: bool,
    /// A line is drawn above: the entry starts a group.
    pub starts_group: bool,
    /// It destroys something: drawn in the error color.
    pub is_danger: bool,
}

/// What the menu of the cursor row offers, in groups like the menu of the file
/// panels; `run` is whether the write actions run.
pub fn menu_items(state: &State, run: bool) -> Vec<MenuItem> {
    let unreachable = state
        .cursor()
        .as_deref()
        .and_then(|path| state.reading(path))
        .is_some_and(crate::summary::Reading::is_unreachable);
    let item = |label, key, entry, enabled| MenuItem {
        label,
        key,
        entry,
        enabled,
        starts_group: false,
        is_danger: false,
    };
    let bulk = |label, key, action| item(label, MenuKey::Own(key), MenuEntry::Bulk(action), run);
    let own = |shortcut: Shortcut, label, entry, enabled| {
        item(label, MenuKey::Own(shortcut), entry, enabled)
    };
    let mut groups = vec![
        vec![item(
            Msg::ReposOpen,
            MenuKey::AppOpen,
            MenuEntry::Open,
            !unreachable,
        )],
        vec![
            bulk(Msg::GitViewToolbarFetch, Shortcut::Fetch, BulkAction::Fetch),
            bulk(
                Msg::ReposActionSwitchDefault,
                Shortcut::SwitchToDefault,
                BulkAction::SwitchToDefault,
            ),
            bulk(
                Msg::ReposActionPullFf,
                Shortcut::PullFastForward,
                BulkAction::PullFastForward,
            ),
            bulk(Msg::ReposActionBoth, Shortcut::Both, BulkAction::Both),
            bulk(Msg::GitViewToolbarPush, Shortcut::Push, BulkAction::Push),
            own(
                Shortcut::CleanUp,
                Msg::ReposToolbarCleanup,
                MenuEntry::Cleanup,
                run,
            ),
        ],
        vec![
            own(
                Shortcut::ShowInFiles,
                Msg::ReposShowInFiles,
                MenuEntry::ShowInFiles,
                !unreachable,
            ),
            item(
                Msg::ContextMenuOpenTerminal,
                MenuKey::AppTerminal,
                MenuEntry::OpenTerminal,
                !unreachable,
            ),
            item(
                Msg::ContextMenuCopyPath,
                MenuKey::AppCopyPath,
                MenuEntry::CopyPath,
                true,
            ),
        ],
        vec![
            own(
                Shortcut::Favourite,
                Msg::ReposToggleFavourite,
                MenuEntry::Favourite,
                true,
            ),
            own(
                Shortcut::OpenInTabs,
                Msg::ReposOpenInTabs,
                MenuEntry::OpenInTabs,
                true,
            ),
        ],
    ];
    if unreachable {
        let mut remove = own(Shortcut::Remove, Msg::ReposRemove, MenuEntry::Remove, true);
        remove.is_danger = true;
        groups.push(vec![remove]);
    }
    groups
        .into_iter()
        .flat_map(|group| {
            group.into_iter().enumerate().map(|(place, mut entry)| {
                entry.starts_group = place == 0;
                entry
            })
        })
        .enumerate()
        .map(|(place, mut entry)| {
            entry.starts_group &= place > 0;
            entry
        })
        .collect()
}

/// The next entry that can be picked from `from` in `step` direction, round
/// the end; `from` itself when no other can.
pub fn next_enabled(items: &[MenuItem], from: usize, step: isize) -> usize {
    let count = items.len();
    (1..=count)
        .map(|distance| {
            (from as isize + step * distance as isize).rem_euclid(count.max(1) as isize) as usize
        })
        .find(|&index| items.get(index).is_some_and(|item| item.enabled))
        .unwrap_or(from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cursor_skips_what_cannot_be_picked_and_goes_round() {
        let items: Vec<MenuItem> = [false, true, false, true]
            .into_iter()
            .map(|enabled| MenuItem {
                label: Msg::ReposOpen,
                key: MenuKey::AppOpen,
                entry: MenuEntry::Open,
                enabled,
                starts_group: false,
                is_danger: false,
            })
            .collect();
        assert_eq!(next_enabled(&items, 1, 1), 3);
        assert_eq!(next_enabled(&items, 3, 1), 1);
        assert_eq!(next_enabled(&items, 1, -1), 3);
        assert_eq!(next_enabled(&items, 0, 1), 1);
    }
}
