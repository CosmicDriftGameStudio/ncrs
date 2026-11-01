//! The context menu of a panel row: which entries it has and which are usable.

use iced::{Point, Size};

use ncrs_i18n::Msg;
use ncrs_ui_kit::layout::{
    place, MENU_ITEM_HEIGHT, MENU_PADDING, MENU_SEPARATOR_HEIGHT, MENU_SUB_WIDTH, MENU_WIDTH,
};

/// What an entry does. Each one is exactly the message its key sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextAction {
    Open,
    View,
    Edit,
    Copy,
    Move,
    NewFolder,
    CopyPath,
    CopyName,
    ComputeFolderSizes,
    Trash,
    DeletePermanently,
    /// Only in the menu of a search-hit list.
    ShowOnOtherSide,
    /// Only in the menu of a search-hit list.
    RemoveFromList,
}

impl ContextAction {
    #[cfg(any(test, feature = "test-support"))]
    pub const ALL: [ContextAction; 13] = [
        ContextAction::Open,
        ContextAction::View,
        ContextAction::Edit,
        ContextAction::Copy,
        ContextAction::Move,
        ContextAction::NewFolder,
        ContextAction::CopyPath,
        ContextAction::CopyName,
        ContextAction::ComputeFolderSizes,
        ContextAction::Trash,
        ContextAction::DeletePermanently,
        ContextAction::ShowOnOtherSide,
        ContextAction::RemoveFromList,
    ];

    /// Whether a panel inside an archive may run the entry; see
    /// `KeyAction::allowed_in_archive`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn allowed_in_archive(self) -> bool {
        match self {
            ContextAction::Open
            | ContextAction::Copy
            | ContextAction::CopyPath
            | ContextAction::CopyName
            | ContextAction::Trash
            | ContextAction::DeletePermanently
            | ContextAction::View
            // Only a hit list offers these, and that is neither.
            | ContextAction::ShowOnOtherSide
            | ContextAction::RemoveFromList => true,
            ContextAction::Edit
            | ContextAction::Move
            | ContextAction::NewFolder
            | ContextAction::ComputeFolderSizes => false,
        }
    }

    /// Whether a panel on an SFTP server may run the entry. See
    /// `KeyAction::allowed_on_server`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn allowed_on_server(self) -> bool {
        match self {
            ContextAction::Open
            | ContextAction::Copy
            | ContextAction::CopyPath
            | ContextAction::CopyName
            | ContextAction::View
            | ContextAction::ShowOnOtherSide
            | ContextAction::RemoveFromList => true,
            ContextAction::Edit
            | ContextAction::Move
            | ContextAction::NewFolder
            | ContextAction::ComputeFolderSizes
            | ContextAction::Trash
            | ContextAction::DeletePermanently => false,
        }
    }

    pub fn label(self) -> Msg {
        match self {
            ContextAction::Open => Msg::ContextMenuOpen,
            ContextAction::View => Msg::ContextMenuView,
            ContextAction::Edit => Msg::ContextMenuEdit,
            ContextAction::Copy => Msg::ContextMenuCopy,
            ContextAction::Move => Msg::ContextMenuMove,
            ContextAction::NewFolder => Msg::ContextMenuNewFolder,
            ContextAction::CopyPath => Msg::ContextMenuCopyPath,
            ContextAction::CopyName => Msg::ContextMenuCopyName,
            ContextAction::ComputeFolderSizes => Msg::ContextMenuComputeFolderSizes,
            ContextAction::Trash => Msg::ContextMenuTrash,
            ContextAction::DeletePermanently => Msg::ContextMenuDeletePermanently,
            ContextAction::ShowOnOtherSide => Msg::ContextMenuShowOnOtherSide,
            ContextAction::RemoveFromList => Msg::ContextMenuRemoveFromList,
        }
    }
}

pub const GROUPS: [&[ContextAction]; 5] = [
    &[
        ContextAction::Open,
        ContextAction::View,
        ContextAction::Edit,
    ],
    &[
        ContextAction::Copy,
        ContextAction::Move,
        ContextAction::NewFolder,
    ],
    &[ContextAction::CopyPath, ContextAction::CopyName],
    &[ContextAction::ComputeFolderSizes],
    &[ContextAction::Trash, ContextAction::DeletePermanently],
];

/// The entries of the menu of a search-hit list: no new folder and no folder
/// sizes, but showing the file in its folder and taking it off the list.
const HIT_LIST_GROUPS: [&[ContextAction]; 5] = [
    &[
        ContextAction::Open,
        ContextAction::View,
        ContextAction::Edit,
    ],
    &[ContextAction::Copy, ContextAction::Move],
    &[
        ContextAction::ShowOnOtherSide,
        ContextAction::RemoveFromList,
    ],
    &[ContextAction::CopyPath, ContextAction::CopyName],
    &[ContextAction::Trash, ContextAction::DeletePermanently],
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuEntry {
    Separator,
    Item {
        action: ContextAction,
        enabled: bool,
    },
    /// The entry that opens the Hermes submenu.
    Hermes,
}

impl MenuEntry {
    fn is_usable(self) -> bool {
        matches!(
            self,
            MenuEntry::Item { enabled: true, .. } | MenuEntry::Hermes
        )
    }
}

/// What an entry of the Hermes submenu does with the file under the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HermesAction {
    ShowSuggestion,
    HandToInbox,
    ProcessAgain,
    ReprocessFolder,
    SetInbox,
    SetFilingLocation,
    SetArchive,
    Exclude,
    Include,
    Inspect,
}

impl HermesAction {
    #[cfg(any(test, feature = "test-support"))]
    pub const ALL: [HermesAction; 10] = [
        HermesAction::ShowSuggestion,
        HermesAction::HandToInbox,
        HermesAction::ProcessAgain,
        HermesAction::ReprocessFolder,
        HermesAction::SetInbox,
        HermesAction::SetFilingLocation,
        HermesAction::SetArchive,
        HermesAction::Exclude,
        HermesAction::Include,
        HermesAction::Inspect,
    ];

    /// Every Hermes entry works on the file or folder under the cursor, whose
    /// path inside an archive is no place Hermes knows.
    #[cfg(any(test, feature = "test-support"))]
    pub fn allowed_in_archive(self) -> bool {
        match self {
            HermesAction::ShowSuggestion
            | HermesAction::HandToInbox
            | HermesAction::ProcessAgain
            | HermesAction::ReprocessFolder
            | HermesAction::SetInbox
            | HermesAction::SetFilingLocation
            | HermesAction::SetArchive
            | HermesAction::Exclude
            | HermesAction::Include
            | HermesAction::Inspect => false,
        }
    }

    /// Hermes knows no place on a server either.
    #[cfg(any(test, feature = "test-support"))]
    pub fn allowed_on_server(self) -> bool {
        match self {
            HermesAction::ShowSuggestion
            | HermesAction::HandToInbox
            | HermesAction::ProcessAgain
            | HermesAction::ReprocessFolder
            | HermesAction::SetInbox
            | HermesAction::SetFilingLocation
            | HermesAction::SetArchive
            | HermesAction::Exclude
            | HermesAction::Include
            | HermesAction::Inspect => false,
        }
    }

    pub fn label(self) -> Msg {
        match self {
            HermesAction::ShowSuggestion => Msg::HermesMenuShowSuggestion,
            HermesAction::HandToInbox => Msg::HermesMenuHandToInbox,
            HermesAction::ProcessAgain => Msg::HermesMenuProcessAgain,
            HermesAction::ReprocessFolder => Msg::HermesMenuReprocessFolder,
            HermesAction::SetInbox => Msg::HermesMenuSetInbox,
            HermesAction::SetFilingLocation => Msg::HermesMenuSetFilingLocation,
            HermesAction::SetArchive => Msg::HermesMenuSetArchive,
            HermesAction::Exclude => Msg::HermesMenuExclude,
            HermesAction::Include => Msg::HermesMenuInclude,
            HermesAction::Inspect => Msg::HermesMenuInspect,
        }
    }

    /// The dimmed word after the label.
    pub fn note(self) -> Option<Msg> {
        match self {
            HermesAction::HandToInbox => Some(Msg::HermesMenuNoteInbox),
            HermesAction::SetArchive => Some(Msg::HermesMenuNoteReadOnly),
            HermesAction::ShowSuggestion
            | HermesAction::ProcessAgain
            | HermesAction::ReprocessFolder
            | HermesAction::SetInbox
            | HermesAction::SetFilingLocation
            | HermesAction::Exclude
            | HermesAction::Include
            | HermesAction::Inspect => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubEntry {
    Separator,
    Item { action: HermesAction, enabled: bool },
}

impl SubEntry {
    fn is_usable(self) -> bool {
        matches!(self, SubEntry::Item { enabled: true, .. })
    }
}

/// How the Hermes entry is built: which of its commands apply to the file.
pub struct HermesMenu<'a> {
    /// The file is excluded, so the menu offers to include it.
    pub excluded: bool,
    pub is_enabled: &'a dyn Fn(HermesAction) -> bool,
}

impl std::fmt::Debug for HermesMenu<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HermesMenu")
            .field("excluded", &self.excluded)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Submenu {
    entries: Vec<SubEntry>,
    selected: usize,
    open: bool,
}

fn sub_groups(excluded: bool) -> [Vec<HermesAction>; 4] {
    [
        vec![HermesAction::ShowSuggestion],
        vec![
            HermesAction::HandToInbox,
            HermesAction::ProcessAgain,
            HermesAction::ReprocessFolder,
        ],
        vec![
            HermesAction::SetInbox,
            HermesAction::SetFilingLocation,
            HermesAction::SetArchive,
        ],
        vec![
            if excluded {
                HermesAction::Include
            } else {
                HermesAction::Exclude
            },
            HermesAction::Inspect,
        ],
    ]
}

/// The usable index after (or before, for a negative `delta`) `from`, going
/// round: past either end it continues at the other one. Stays at `from` when
/// no other entry can be used.
fn next_usable_index(is_usable: &[bool], from: usize, delta: isize) -> usize {
    let count = is_usable.len();
    (1..count)
        .map(|step| {
            if delta < 0 {
                (from + count - step % count) % count
            } else {
                (from + step) % count
            }
        })
        .find(|&index| is_usable.get(index).copied().unwrap_or(false))
        .unwrap_or(from)
}

fn step_usable(entries: &[SubEntry], from: usize, delta: isize) -> usize {
    let usable: Vec<bool> = entries.iter().map(|entry| entry.is_usable()).collect();
    next_usable_index(&usable, from, delta)
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContextMenu {
    origin: Point,
    entries: Vec<MenuEntry>,
    /// Index into `entries`; always a usable item.
    selected: usize,
    window: Size,
    hermes: Option<Submenu>,
}

impl ContextMenu {
    /// A menu next to `anchor` that stays inside `window`: it opens to the
    /// right and below, and folds to the left or up where there is no room.
    #[cfg(any(test, feature = "test-support"))]
    pub fn new(anchor: Point, window: Size, is_enabled: impl Fn(ContextAction) -> bool) -> Self {
        Self::with_hermes(anchor, window, is_enabled, None)
    }

    /// As [`Self::new`], with the Hermes entry after the file groups when
    /// `hermes` says the column is up.
    pub fn with_hermes(
        anchor: Point,
        window: Size,
        is_enabled: impl Fn(ContextAction) -> bool,
        hermes: Option<HermesMenu<'_>>,
    ) -> Self {
        Self::of_groups(&GROUPS, &[], anchor, window, is_enabled, hermes)
    }

    /// The menu of a row of a search-hit list.
    pub fn for_hit_list(
        anchor: Point,
        window: Size,
        is_enabled: impl Fn(ContextAction) -> bool,
        hermes: Option<HermesMenu<'_>>,
    ) -> Self {
        Self::of_groups(&HIT_LIST_GROUPS, &[], anchor, window, is_enabled, hermes)
    }

    /// The menu of a row of the search results: the hit list's, without
    /// taking the file off the list, since a new search rebuilds the results.
    pub fn for_search_results(
        anchor: Point,
        window: Size,
        is_enabled: impl Fn(ContextAction) -> bool,
    ) -> Self {
        Self::of_groups(
            &HIT_LIST_GROUPS,
            &[ContextAction::RemoveFromList],
            anchor,
            window,
            is_enabled,
            None,
        )
    }

    fn of_groups(
        groups: &[&[ContextAction]],
        left_out: &[ContextAction],
        anchor: Point,
        window: Size,
        is_enabled: impl Fn(ContextAction) -> bool,
        hermes: Option<HermesMenu<'_>>,
    ) -> Self {
        let mut entries = Vec::new();
        for (group_index, group) in groups.iter().enumerate() {
            if group_index > 0 {
                entries.push(MenuEntry::Separator);
            }
            entries.extend(
                group
                    .iter()
                    .filter(|action| !left_out.contains(action))
                    .map(|&action| MenuEntry::Item {
                        action,
                        enabled: is_enabled(action),
                    }),
            );
        }
        let submenu = hermes.map(|menu| {
            entries.push(MenuEntry::Separator);
            entries.push(MenuEntry::Hermes);
            Submenu::new(&menu)
        });
        let selected = entries
            .iter()
            .position(|entry| entry.is_usable())
            .unwrap_or(0);
        let size = Self::size_of(&entries);
        Self {
            origin: Point::new(
                place(anchor.x, size.width, window.width),
                place(anchor.y, size.height, window.height),
            ),
            entries,
            selected,
            window,
            hermes: submenu,
        }
    }

    pub fn size_of(entries: &[MenuEntry]) -> Size {
        let height: f32 = entries
            .iter()
            .map(|entry| match entry {
                MenuEntry::Separator => MENU_SEPARATOR_HEIGHT,
                MenuEntry::Item { .. } | MenuEntry::Hermes => MENU_ITEM_HEIGHT,
            })
            .sum();
        Size::new(MENU_WIDTH, height + 2.0 * MENU_PADDING)
    }

    pub fn origin(&self) -> Point {
        self.origin
    }

    pub fn entries(&self) -> &[MenuEntry] {
        &self.entries
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn selected_action(&self) -> Option<ContextAction> {
        match self.entries.get(self.selected) {
            Some(MenuEntry::Item {
                action,
                enabled: true,
            }) => Some(*action),
            _ => None,
        }
    }

    /// Highlights the entry at `index` if it can be used.
    pub fn select(&mut self, index: usize) {
        if self
            .entries
            .get(index)
            .is_some_and(|entry| entry.is_usable())
        {
            self.selected = index;
        }
    }

    /// The pointer is over the entry at `index`: the Hermes entry opens its
    /// submenu, any other closes it.
    pub fn hover(&mut self, index: usize) {
        self.select(index);
        if self.selected == index {
            if self.selected_is_hermes() {
                self.open_submenu();
            } else {
                self.close_submenu();
            }
        }
    }

    /// Moves to the next usable entry in the direction of `delta`, skipping
    /// separators and greyed-out ones, and goes round at either end. In the
    /// open submenu it moves there.
    pub fn move_by(&mut self, delta: isize) {
        if let Some(sub) = self.open_sub_mut() {
            sub.selected = step_usable(&sub.entries, sub.selected, delta);
            return;
        }
        let usable: Vec<bool> = self.entries.iter().map(|entry| entry.is_usable()).collect();
        self.selected = next_usable_index(&usable, self.selected, delta);
    }

    pub fn select_first(&mut self) {
        if let Some(sub) = self.open_sub_mut() {
            sub.selected = sub.first_usable();
            return;
        }
        if let Some(index) = self.entries.iter().position(|entry| entry.is_usable()) {
            self.selected = index;
        }
    }

    pub fn select_last(&mut self) {
        if let Some(sub) = self.open_sub_mut() {
            sub.selected = sub
                .entries
                .iter()
                .rposition(|entry| entry.is_usable())
                .unwrap_or(sub.selected);
            return;
        }
        if let Some(index) = self.entries.iter().rposition(|entry| entry.is_usable()) {
            self.selected = index;
        }
    }

    fn open_sub_mut(&mut self) -> Option<&mut Submenu> {
        self.hermes.as_mut().filter(|sub| sub.open)
    }

    pub fn selected_is_hermes(&self) -> bool {
        matches!(self.entries.get(self.selected), Some(MenuEntry::Hermes))
    }

    pub fn submenu_is_open(&self) -> bool {
        self.hermes.as_ref().is_some_and(|sub| sub.open)
    }

    /// Opens the submenu of the highlighted Hermes entry, with its first
    /// usable entry highlighted. False when the highlight is on another entry.
    pub fn open_submenu(&mut self) -> bool {
        if !self.selected_is_hermes() {
            return false;
        }
        match self.hermes.as_mut() {
            Some(sub) => {
                if !sub.open {
                    sub.selected = sub.first_usable();
                }
                sub.open = true;
                true
            }
            None => false,
        }
    }

    /// True when a submenu was open.
    pub fn close_submenu(&mut self) -> bool {
        match self.hermes.as_mut() {
            Some(sub) if sub.open => {
                sub.open = false;
                true
            }
            _ => false,
        }
    }

    pub fn sub_entries(&self) -> &[SubEntry] {
        self.hermes
            .as_ref()
            .map_or(&[], |sub| sub.entries.as_slice())
    }

    pub fn sub_selected(&self) -> usize {
        self.hermes.as_ref().map_or(0, |sub| sub.selected)
    }

    /// Highlights the entry of the open submenu at `index` if it can be used.
    pub fn select_sub(&mut self, index: usize) {
        if let Some(sub) = self.open_sub_mut() {
            if sub
                .entries
                .get(index)
                .is_some_and(|entry| entry.is_usable())
            {
                sub.selected = index;
            }
        }
    }

    pub fn selected_hermes_action(&self) -> Option<HermesAction> {
        let sub = self.hermes.as_ref().filter(|sub| sub.open)?;
        match sub.entries.get(sub.selected) {
            Some(SubEntry::Item {
                action,
                enabled: true,
            }) => Some(*action),
            _ => None,
        }
    }

    /// Where the open submenu goes: beside the Hermes entry, to the right or,
    /// where that has no room, to the left, and up as far as it must to stay
    /// in the window. `None` while it is closed.
    pub fn submenu_origin(&self) -> Option<Point> {
        let sub = self.hermes.as_ref().filter(|sub| sub.open)?;
        let height = sub_height(&sub.entries);
        let row_top = self.origin.y
            + MENU_PADDING
            + self
                .entries
                .iter()
                .take(self.selected)
                .map(entry_height)
                .sum::<f32>();
        let right = self.origin.x + MENU_WIDTH;
        let x = if right + MENU_SUB_WIDTH > self.window.width {
            self.origin.x - MENU_SUB_WIDTH
        } else {
            right
        };
        Some(Point::new(
            x.max(0.0),
            place(row_top - MENU_PADDING, height, self.window.height),
        ))
    }
}

impl Submenu {
    fn new(menu: &HermesMenu<'_>) -> Self {
        let mut entries = Vec::new();
        for (group_index, group) in sub_groups(menu.excluded).iter().enumerate() {
            if group_index > 0 {
                entries.push(SubEntry::Separator);
            }
            entries.extend(group.iter().map(|&action| SubEntry::Item {
                action,
                enabled: (menu.is_enabled)(action),
            }));
        }
        let mut sub = Self {
            entries,
            selected: 0,
            open: false,
        };
        sub.selected = sub.first_usable();
        sub
    }

    fn first_usable(&self) -> usize {
        self.entries
            .iter()
            .position(|entry| entry.is_usable())
            .unwrap_or(0)
    }
}

fn entry_height(entry: &MenuEntry) -> f32 {
    match entry {
        MenuEntry::Separator => MENU_SEPARATOR_HEIGHT,
        MenuEntry::Item { .. } | MenuEntry::Hermes => MENU_ITEM_HEIGHT,
    }
}

pub fn sub_height(entries: &[SubEntry]) -> f32 {
    let inner: f32 = entries
        .iter()
        .map(|entry| match entry {
            SubEntry::Separator => MENU_SEPARATOR_HEIGHT,
            SubEntry::Item { .. } => MENU_ITEM_HEIGHT,
        })
        .sum();
    inner + 2.0 * MENU_PADDING
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Size = Size::new(1200.0, 760.0);

    fn menu_with(disabled: &[ContextAction]) -> ContextMenu {
        ContextMenu::new(Point::new(10.0, 10.0), WINDOW, |action| {
            !disabled.contains(&action)
        })
    }

    fn selected_action(menu: &ContextMenu) -> ContextAction {
        menu.selected_action().unwrap()
    }

    #[test]
    fn the_menu_has_the_five_groups_with_separators() {
        let menu = menu_with(&[]);
        let separators = menu
            .entries()
            .iter()
            .filter(|entry| **entry == MenuEntry::Separator)
            .count();
        assert_eq!(separators, 4);
        assert_eq!(menu.entries().len(), 11 + 4);
        assert_eq!(selected_action(&menu), ContextAction::Open);
    }

    #[test]
    fn navigation_skips_separators_and_greyed_out_entries() {
        let mut menu = menu_with(&[ContextAction::View, ContextAction::Edit]);
        menu.move_by(1);
        assert_eq!(selected_action(&menu), ContextAction::Copy);
        menu.move_by(-1);
        assert_eq!(selected_action(&menu), ContextAction::Open);
        menu.move_by(-1);
        assert_eq!(
            selected_action(&menu),
            ContextAction::DeletePermanently,
            "up from the top goes round to the last entry"
        );
        menu.move_by(1);
        assert_eq!(
            selected_action(&menu),
            ContextAction::Open,
            "down from the bottom goes round to the first entry"
        );
        menu.select_last();
        assert_eq!(selected_action(&menu), ContextAction::DeletePermanently);
        menu.move_by(-1);
        assert_eq!(selected_action(&menu), ContextAction::Trash);
        menu.move_by(-1);
        assert_eq!(selected_action(&menu), ContextAction::ComputeFolderSizes);
        menu.move_by(-1);
        assert_eq!(selected_action(&menu), ContextAction::CopyName);
        menu.select_first();
        assert_eq!(selected_action(&menu), ContextAction::Open);
    }

    #[test]
    fn going_round_in_the_menu_skips_greyed_out_entries_at_both_ends() {
        let mut menu = menu_with(&[ContextAction::Open, ContextAction::DeletePermanently]);
        let first = selected_action(&menu);
        assert_ne!(first, ContextAction::Open);
        menu.move_by(-1);
        assert_eq!(selected_action(&menu), ContextAction::Trash);
        menu.move_by(1);
        assert_eq!(selected_action(&menu), first);
    }

    #[test]
    fn going_round_in_the_submenu_skips_greyed_out_entries_at_both_ends() {
        let mut menu = hermes_menu(&[HermesAction::ShowSuggestion, HermesAction::Inspect]);
        menu.select_last();
        menu.open_submenu();
        menu.select_first();
        let first = menu.selected_hermes_action();
        assert_eq!(first, Some(HermesAction::HandToInbox));
        menu.move_by(-1);
        assert_eq!(menu.selected_hermes_action(), Some(HermesAction::Exclude));
        menu.move_by(1);
        assert_eq!(menu.selected_hermes_action(), first);
    }

    #[test]
    fn going_round_skips_greyed_out_entries_at_both_ends() {
        let usable = [false, true, false, true, false];
        assert_eq!(next_usable_index(&usable, 3, 1), 1, "past the end");
        assert_eq!(next_usable_index(&usable, 1, -1), 3, "before the start");
        assert_eq!(next_usable_index(&usable, 1, 1), 3);
        assert_eq!(next_usable_index(&[true], 0, 1), 0, "alone");
        assert_eq!(next_usable_index(&[false, false], 1, -1), 1, "none usable");
        assert_eq!(next_usable_index(&[], 0, 1), 0, "empty");
    }

    #[test]
    fn only_usable_entries_can_be_highlighted() {
        let mut menu = menu_with(&[ContextAction::View]);
        menu.select(1);
        assert_eq!(selected_action(&menu), ContextAction::Open);
        menu.select(4);
        assert_eq!(selected_action(&menu), ContextAction::Copy);
        let separator = menu
            .entries()
            .iter()
            .position(|entry| *entry == MenuEntry::Separator)
            .unwrap();
        menu.select(separator);
        assert_eq!(selected_action(&menu), ContextAction::Copy);
    }

    #[test]
    fn the_first_usable_entry_is_highlighted_when_open_is_unusable() {
        let menu = menu_with(&[ContextAction::Open, ContextAction::View]);
        assert_eq!(selected_action(&menu), ContextAction::Edit);
    }

    #[test]
    fn the_menu_folds_back_into_the_window() {
        let size = ContextMenu::size_of(menu_with(&[]).entries());
        let corner = ContextMenu::new(
            Point::new(WINDOW.width - 5.0, WINDOW.height - 5.0),
            WINDOW,
            |_| true,
        );
        assert_eq!(
            corner.origin(),
            Point::new(
                WINDOW.width - 5.0 - size.width,
                WINDOW.height - 5.0 - size.height
            )
        );
        let inside = ContextMenu::new(Point::new(40.0, 50.0), WINDOW, |_| true);
        assert_eq!(inside.origin(), Point::new(40.0, 50.0));
        let tiny = ContextMenu::new(Point::new(40.0, 50.0), Size::new(100.0, 100.0), |_| true);
        assert_eq!(tiny.origin(), Point::ORIGIN);
    }

    fn hermes_menu(disabled: &'static [HermesAction]) -> ContextMenu {
        let enabled = move |action: HermesAction| !disabled.contains(&action);
        ContextMenu::with_hermes(
            Point::new(10.0, 10.0),
            WINDOW,
            |_| true,
            Some(HermesMenu {
                excluded: false,
                is_enabled: &enabled,
            }),
        )
    }

    #[test]
    fn the_hermes_entry_is_last_and_only_there_with_the_column() {
        let menu = hermes_menu(&[]);
        assert_eq!(menu.entries().last(), Some(&MenuEntry::Hermes));
        assert!(!menu_with(&[]).entries().contains(&MenuEntry::Hermes));
    }

    #[test]
    fn right_opens_the_submenu_on_the_hermes_entry_and_left_closes_it() {
        let mut menu = hermes_menu(&[]);
        assert!(!menu.open_submenu(), "the highlight is on Open");
        menu.select_last();
        assert!(menu.selected_is_hermes());
        assert!(menu.open_submenu());
        assert_eq!(
            menu.selected_hermes_action(),
            Some(HermesAction::ShowSuggestion)
        );
        assert!(menu.close_submenu());
        assert!(!menu.submenu_is_open());
        assert!(!menu.close_submenu());
    }

    #[test]
    fn the_submenu_skips_separators_and_greyed_entries() {
        let mut menu = hermes_menu(&[
            HermesAction::ProcessAgain,
            HermesAction::ReprocessFolder,
            HermesAction::SetInbox,
        ]);
        menu.select_last();
        menu.open_submenu();
        menu.move_by(1);
        assert_eq!(
            menu.selected_hermes_action(),
            Some(HermesAction::HandToInbox)
        );
        menu.move_by(1);
        assert_eq!(
            menu.selected_hermes_action(),
            Some(HermesAction::SetFilingLocation)
        );
        menu.select_last();
        assert_eq!(menu.selected_hermes_action(), Some(HermesAction::Inspect));
        menu.move_by(1);
        assert_eq!(
            menu.selected_hermes_action(),
            Some(HermesAction::ShowSuggestion),
            "down from the last entry goes round to the first"
        );
        menu.move_by(-1);
        assert_eq!(
            menu.selected_hermes_action(),
            Some(HermesAction::Inspect),
            "up from the first entry goes round to the last"
        );
        menu.select_first();
        assert_eq!(
            menu.selected_hermes_action(),
            Some(HermesAction::ShowSuggestion)
        );
        assert!(menu.selected_is_hermes(), "the parent keeps its highlight");
    }

    #[test]
    fn hovering_another_entry_closes_the_submenu() {
        let mut menu = hermes_menu(&[]);
        menu.select_last();
        let hermes = menu.selected();
        menu.hover(hermes);
        assert!(menu.submenu_is_open());
        menu.hover(0);
        assert!(!menu.submenu_is_open());
    }

    #[test]
    fn the_submenu_folds_to_the_left_where_there_is_no_room() {
        let mut menu = hermes_menu(&[]);
        menu.select_last();
        menu.open_submenu();
        let origin = menu.submenu_origin().unwrap();
        assert_eq!(origin.x, menu.origin().x + MENU_WIDTH);

        let mut right = ContextMenu::with_hermes(
            Point::new(WINDOW.width, 10.0),
            WINDOW,
            |_| true,
            Some(HermesMenu {
                excluded: true,
                is_enabled: &|_| true,
            }),
        );
        right.select_last();
        right.open_submenu();
        assert_eq!(
            right.submenu_origin().unwrap().x,
            right.origin().x - MENU_SUB_WIDTH
        );
        assert!(right.sub_entries().iter().any(|entry| matches!(
            entry,
            SubEntry::Item {
                action: HermesAction::Include,
                ..
            }
        )));
    }
}
