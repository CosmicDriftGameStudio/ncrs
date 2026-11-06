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
    /// Not in the menu of a list of hits or of the search results: they have
    /// no folder of their own.
    OpenTerminal,
    Copy,
    Move,
    Rename,
    NewFolder,
    CopyPath,
    CopyName,
    ComputeFolderSizes,
    /// Only for a folder that is not a favourite yet.
    AddFavourite,
    /// Only for a folder that is a favourite.
    RemoveFavourite,
    Trash,
    DeletePermanently,
    /// Only in the menu of a search-hit list.
    ShowOnOtherSide,
    /// Only in the menu of a search-hit list.
    RemoveFromList,
}

impl ContextAction {
    #[cfg(any(test, feature = "test-support"))]
    pub const ALL: [ContextAction; 17] = [
        ContextAction::Open,
        ContextAction::View,
        ContextAction::Edit,
        ContextAction::OpenTerminal,
        ContextAction::Copy,
        ContextAction::Move,
        ContextAction::Rename,
        ContextAction::NewFolder,
        ContextAction::CopyPath,
        ContextAction::CopyName,
        ContextAction::ComputeFolderSizes,
        ContextAction::AddFavourite,
        ContextAction::RemoveFavourite,
        ContextAction::Trash,
        ContextAction::DeletePermanently,
        ContextAction::ShowOnOtherSide,
        ContextAction::RemoveFromList,
    ];

    /// Whether a panel inside an archive may run the entry; see
    /// `KeyAction::allowed_in_archive`.
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
            | ContextAction::OpenTerminal
            | ContextAction::Move
            | ContextAction::Rename
            | ContextAction::NewFolder
            | ContextAction::AddFavourite
            | ContextAction::RemoveFavourite
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
            | ContextAction::Rename
            | ContextAction::ShowOnOtherSide
            | ContextAction::RemoveFromList => true,
            ContextAction::Edit
            | ContextAction::OpenTerminal
            | ContextAction::Move
            | ContextAction::NewFolder
            | ContextAction::ComputeFolderSizes
            | ContextAction::AddFavourite
            | ContextAction::RemoveFavourite
            | ContextAction::Trash
            | ContextAction::DeletePermanently => false,
        }
    }

    pub fn label(self) -> Msg {
        match self {
            ContextAction::Open => Msg::ContextMenuOpen,
            ContextAction::View => Msg::ContextMenuView,
            ContextAction::Edit => Msg::ContextMenuEdit,
            ContextAction::OpenTerminal => Msg::ContextMenuOpenTerminal,
            ContextAction::Copy => Msg::ContextMenuCopy,
            ContextAction::Move => Msg::ContextMenuMove,
            ContextAction::Rename => Msg::ContextMenuRename,
            ContextAction::NewFolder => Msg::ContextMenuNewFolder,
            ContextAction::CopyPath => Msg::ContextMenuCopyPath,
            ContextAction::CopyName => Msg::ContextMenuCopyName,
            ContextAction::ComputeFolderSizes => Msg::ContextMenuComputeFolderSizes,
            ContextAction::AddFavourite => Msg::ContextMenuAddFavourite,
            ContextAction::RemoveFavourite => Msg::ContextMenuRemoveFavourite,
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
        ContextAction::OpenTerminal,
    ],
    &[
        ContextAction::Copy,
        ContextAction::Move,
        ContextAction::Rename,
        ContextAction::NewFolder,
    ],
    &[ContextAction::CopyPath, ContextAction::CopyName],
    &[
        ContextAction::ComputeFolderSizes,
        ContextAction::AddFavourite,
        ContextAction::RemoveFavourite,
    ],
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
    &[
        ContextAction::Copy,
        ContextAction::Move,
        ContextAction::Rename,
    ],
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
    SetInbox,
    SetFilingLocation,
    SetArchive,
    Exclude,
    Include,
    Inspect,
}

impl HermesAction {
    #[cfg(any(test, feature = "test-support"))]
    pub const ALL: [HermesAction; 9] = [
        HermesAction::ShowSuggestion,
        HermesAction::HandToInbox,
        HermesAction::ProcessAgain,
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
        vec![HermesAction::HandToInbox, HermesAction::ProcessAgain],
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
        Self::with_favourite(anchor, window, is_enabled, hermes, None)
    }

    /// As [`Self::with_hermes`], with the entry of the favourites that fits
    /// the folder: `Some(true)` for one that is a favourite (the entry removes
    /// it), `Some(false)` for one that is not, `None` for a place that has no
    /// favourites, which leaves both entries out.
    pub fn with_favourite(
        anchor: Point,
        window: Size,
        is_enabled: impl Fn(ContextAction) -> bool,
        hermes: Option<HermesMenu<'_>>,
        is_favourite: Option<bool>,
    ) -> Self {
        let left_out: &[ContextAction] = match is_favourite {
            Some(true) => &[ContextAction::AddFavourite],
            Some(false) => &[ContextAction::RemoveFavourite],
            None => &[ContextAction::AddFavourite, ContextAction::RemoveFavourite],
        };
        Self::of_groups(&GROUPS, left_out, anchor, window, is_enabled, hermes)
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

#[cfg(test)]
mod tests;
