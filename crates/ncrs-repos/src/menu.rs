//! The entries of the menu of a row: the view draws them, the keys walk them.

use ncrs_i18n::Msg;

use crate::bulk::BulkAction;
use crate::message::MenuEntry;
use crate::shortcuts::Shortcut;
use crate::state::State;

pub(crate) struct MenuItem {
    pub(crate) label: Msg,
    pub(crate) key: Option<Shortcut>,
    pub(crate) entry: MenuEntry,
    pub(crate) enabled: bool,
}

/// What the menu of the cursor row offers; `run` is whether the write actions run.
pub(crate) fn menu_items(state: &State, run: bool) -> Vec<MenuItem> {
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
    };
    let mut items = vec![
        item(Msg::ReposOpen, None, MenuEntry::Open, !unreachable),
        item(
            Msg::ReposToggleFavourite,
            Some(Shortcut::Favourite),
            MenuEntry::Favourite,
            true,
        ),
    ];
    if unreachable {
        items.push(item(
            Msg::ReposRemove,
            Some(Shortcut::Remove),
            MenuEntry::Remove,
            true,
        ));
    }
    let bulk = |label, key, action| item(label, Some(key), MenuEntry::Bulk(action), run);
    items.extend([
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
        item(
            Msg::ReposToolbarCleanup,
            Some(Shortcut::CleanUp),
            MenuEntry::Cleanup,
            run,
        ),
        item(
            Msg::ReposOpenInTabs,
            Some(Shortcut::OpenInTabs),
            MenuEntry::OpenInTabs,
            true,
        ),
    ]);
    items
}

/// The next entry that can be picked from `from` in `step` direction, round
/// the end; `from` itself when no other can.
pub(crate) fn next_enabled(items: &[MenuItem], from: usize, step: isize) -> usize {
    let count = items.len();
    (1..=count)
        .map(|distance| (from as isize + step * distance as isize).rem_euclid(count.max(1) as isize) as usize)
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
                key: None,
                entry: MenuEntry::Open,
                enabled,
            })
            .collect();
        assert_eq!(next_enabled(&items, 1, 1), 3);
        assert_eq!(next_enabled(&items, 3, 1), 1);
        assert_eq!(next_enabled(&items, 1, -1), 3);
        assert_eq!(next_enabled(&items, 0, 1), 1);
    }
}
