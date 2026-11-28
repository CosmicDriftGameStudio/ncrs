//! The decide pane (#379): the items of Downloads sorted by reason, or the items of a big
//! folder, and the choice of what goes to the trash. Nothing here deletes; the trash run
//! is an action of the app, so undo brings the files back.

mod side;
mod update;

use std::collections::HashSet;
use std::path::PathBuf;

use ncrs_filing::execute::OwnTrashItem;
use ncrs_space::decide::{DownloadGroups, DownloadItem};
use ncrs_ui_kit::kit::Check;

pub use ncrs_space::InstalledApp;

pub use side::update as side_update;
pub use update::{open, reload, update};

/// The id of the scrollable around the items.
pub const DECIDE_LIST_ID: &str = "space-decide-list";
/// The id of the scrollable around the overview beside the list.
pub const DECIDE_SIDE_ID: &str = "space-decide-side";

/// A group with more rows than this shows only [`ROWS_SHOWN`] of them and a row for the rest.
const ROWS_BEFORE_MORE: usize = 10;
const ROWS_SHOWN: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DecideGroupKind {
    Installed,
    Unpacked,
    Duplicates,
    UntouchedYear,
    Rest,
}

impl DecideGroupKind {
    /// Only what exists somewhere else is chosen from the start.
    pub const fn is_chosen_from_the_start(self) -> bool {
        matches!(self, Self::Installed | Self::Unpacked | Self::Duplicates)
    }
}

/// Why an item is offered; the checks come from facts the scan found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecideReason {
    /// The app the installer is for is installed.
    Installed(InstalledApp),
    /// The archive is unpacked into this folder.
    Unpacked(PathBuf),
    /// The same content lies at this path, which stays.
    Duplicate(PathBuf),
    Untouched,
    Rest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecideItem {
    pub item: DownloadItem,
    pub reason: DecideReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecideGroup {
    pub kind: DecideGroupKind,
    pub items: Vec<DecideItem>,
}

/// The parts of the pane the keys can be in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DecideArea {
    /// The narrow overview on the left.
    Side,
    #[default]
    List,
    Detail,
}

impl DecideArea {
    pub const fn next(self) -> Self {
        match self {
            Self::Side => Self::List,
            Self::List => Self::Detail,
            Self::Detail => Self::Side,
        }
    }

    pub const fn previous(self) -> Self {
        match self {
            Self::Side => Self::Detail,
            Self::List => Self::Side,
            Self::Detail => Self::List,
        }
    }
}

/// One stop of the keyboard in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecideKey {
    Head(DecideGroupKind),
    Item(PathBuf),
    /// The row that opens the rest of a long group.
    More(DecideGroupKind),
}

/// What the pane tells after a trash run or an undo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecideNotice {
    Trashed {
        count: usize,
        bytes: u64,
        /// The trash was emptied right after: the space is free already.
        emptied: bool,
    },
    Undone {
        count: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecideMessage {
    /// The folder was read and sorted.
    Loaded {
        folder: PathBuf,
        groups: Box<DownloadGroups>,
    },
    Up,
    Down,
    PageUp,
    PageDown,
    First,
    Last,
    Toggle,
    Enter,
    /// Tab: the keys go to the next part of the pane.
    NextArea,
    PreviousArea,
    RowPressed(PathBuf),
    RowToggled(PathBuf),
    HeadToggled(DecideGroupKind),
    /// The tick of a group head: all of the group, or none of it.
    GroupToggled(DecideGroupKind),
    /// The row at the end of a long group: shows the rest of it.
    MoreOpened(DecideGroupKind),
    /// A row of the overview beside the list: opens it when it is a folder to decide on.
    SidePressed(PathBuf),
    /// Puts the chosen files in the trash.
    Trash,
    Undo,
    ToggleEmptyTrash,
    /// The app put these files in the trash.
    Trashed(Vec<PathBuf>),
    /// The app brought the last trash run back: this many files.
    Undone(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecideState {
    pub folder: PathBuf,
    pub is_downloads: bool,
    pub groups: Vec<DecideGroup>,
    pub loaded: bool,
    pub chosen: HashSet<PathBuf>,
    pub folded: HashSet<DecideGroupKind>,
    pub expanded: HashSet<DecideGroupKind>,
    pub cursor: Option<DecideKey>,
    pub area: DecideArea,
    /// The row of the overview under the keys.
    pub side_cursor: PathBuf,
    pub empty_after: bool,
    pub emptying: bool,
    /// A trash run is under way: a second press waits for its answer.
    pub trashing: bool,
    pub notice: Option<DecideNotice>,
    pub can_undo: bool,
}

impl DecideState {
    pub(crate) fn new(folder: PathBuf, is_downloads: bool) -> Self {
        Self {
            side_cursor: folder.clone(),
            folder,
            is_downloads,
            groups: Vec::new(),
            loaded: false,
            chosen: HashSet::new(),
            folded: HashSet::new(),
            expanded: HashSet::new(),
            cursor: None,
            area: DecideArea::default(),
            empty_after: false,
            emptying: false,
            trashing: false,
            notice: None,
            can_undo: false,
        }
    }

    pub fn folder(&self) -> &std::path::Path {
        &self.folder
    }

    pub const fn is_downloads(&self) -> bool {
        self.is_downloads
    }

    pub const fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn groups(&self) -> &[DecideGroup] {
        &self.groups
    }

    pub const fn area(&self) -> DecideArea {
        self.area
    }

    pub fn side_cursor(&self) -> &std::path::Path {
        &self.side_cursor
    }

    pub const fn empty_after(&self) -> bool {
        self.empty_after
    }

    pub const fn notice(&self) -> Option<DecideNotice> {
        self.notice
    }

    pub const fn is_trashing(&self) -> bool {
        self.trashing
    }

    pub const fn can_undo(&self) -> bool {
        self.can_undo
    }

    pub fn is_chosen(&self, path: &std::path::Path) -> bool {
        self.chosen.contains(path)
    }

    pub fn is_folded(&self, kind: DecideGroupKind) -> bool {
        self.folded.contains(&kind)
    }

    /// How many items of the group show before the "more" row.
    pub fn shown_len(&self, group: &DecideGroup) -> usize {
        if self.expanded.contains(&group.kind) || group.items.len() <= ROWS_BEFORE_MORE {
            group.items.len()
        } else {
            ROWS_SHOWN
        }
    }

    /// The items of the group that are not shown: how many and how big.
    pub fn hidden_of(&self, group: &DecideGroup) -> (usize, u64) {
        let hidden = group.items.get(self.shown_len(group)..).unwrap_or_default();
        let bytes = hidden
            .iter()
            .fold(0, |sum: u64, entry| sum.saturating_add(entry.item.size));
        (hidden.len(), bytes)
    }

    /// The stops of the keyboard: a head per group, the shown items of the open ones and the
    /// "more" row of a long group.
    pub(crate) fn keys(&self) -> Vec<DecideKey> {
        let mut keys = Vec::new();
        for group in &self.groups {
            keys.push(DecideKey::Head(group.kind));
            if self.is_folded(group.kind) {
                continue;
            }
            let shown = self.shown_len(group);
            keys.extend(
                group
                    .items
                    .get(..shown)
                    .unwrap_or_default()
                    .iter()
                    .map(|entry| DecideKey::Item(entry.item.path.clone())),
            );
            if shown < group.items.len() {
                keys.push(DecideKey::More(group.kind));
            }
        }
        keys
    }

    pub fn cursor_index(&self) -> Option<usize> {
        let cursor = self.cursor.as_ref()?;
        self.keys().iter().position(|key| key == cursor)
    }

    /// The item under the cursor; none on a head.
    pub fn cursor_item(&self) -> Option<&DecideItem> {
        match self.cursor.as_ref()? {
            DecideKey::Item(path) => self.item(path),
            DecideKey::Head(_) | DecideKey::More(_) => None,
        }
    }

    /// The group the cursor is in or on.
    pub fn cursor_group(&self) -> Option<DecideGroupKind> {
        match self.cursor.as_ref()? {
            DecideKey::Head(kind) | DecideKey::More(kind) => Some(*kind),
            DecideKey::Item(path) => self
                .groups
                .iter()
                .find(|group| group.items.iter().any(|entry| &entry.item.path == path))
                .map(|group| group.kind),
        }
    }

    /// What "show in panel" opens: the item under the cursor, or the folder.
    pub fn panel_path(&self) -> &std::path::Path {
        self.cursor_item()
            .map_or(self.folder.as_path(), |entry| entry.item.path.as_path())
    }

    pub fn item(&self, path: &std::path::Path) -> Option<&DecideItem> {
        self.groups
            .iter()
            .flat_map(|group| group.items.iter())
            .find(|entry| entry.item.path == path)
    }

    pub fn chosen_count(&self) -> usize {
        self.chosen.len()
    }

    pub fn chosen_bytes(&self) -> u64 {
        self.groups
            .iter()
            .flat_map(|group| group.items.iter())
            .filter(|entry| self.chosen.contains(&entry.item.path))
            .fold(0, |sum: u64, entry| sum.saturating_add(entry.item.size))
    }

    pub fn total_bytes(&self) -> u64 {
        self.groups
            .iter()
            .flat_map(|group| group.items.iter())
            .fold(0, |sum: u64, entry| sum.saturating_add(entry.item.size))
    }

    pub fn group_bytes(group: &DecideGroup) -> u64 {
        group
            .items
            .iter()
            .fold(0, |sum: u64, entry| sum.saturating_add(entry.item.size))
    }

    /// Whether all, some or none of the group is chosen.
    pub fn group_check(&self, group: &DecideGroup) -> Check {
        let chosen = group
            .items
            .iter()
            .filter(|entry| self.chosen.contains(&entry.item.path))
            .count();
        match chosen {
            0 => Check::Off,
            all if all == group.items.len() => Check::On,
            _ => Check::Mixed,
        }
    }

    /// How many groups have everything chosen, and how many groups there are.
    pub fn groups_chosen(&self) -> (usize, usize) {
        let chosen = self
            .groups
            .iter()
            .filter(|group| !group.items.is_empty() && self.group_check(group) == Check::On)
            .count();
        (chosen, self.groups.len())
    }

    /// The chosen items as the trash run takes them, in the order of the list.
    pub fn chosen_items(&self) -> Vec<OwnTrashItem> {
        self.groups
            .iter()
            .flat_map(|group| group.items.iter())
            .filter(|entry| self.chosen.contains(&entry.item.path))
            .map(|entry| OwnTrashItem {
                path: entry.item.path.clone(),
                size: entry.item.size,
                modified: entry.item.modified,
            })
            .collect()
    }
}
