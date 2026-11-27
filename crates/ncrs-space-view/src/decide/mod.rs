//! The decide pane (#379): the items of Downloads sorted by reason, or the items of a big
//! folder, and the choice of what goes to the trash. Nothing here deletes; the trash run
//! is an action of the app, so undo brings the files back.

mod update;

use std::collections::HashSet;
use std::path::PathBuf;

use ncrs_filing::execute::OwnTrashItem;
use ncrs_space::decide::{DownloadGroups, DownloadItem};

pub(crate) use update::{open, update};

/// The id of the scrollable around the items.
pub const DECIDE_LIST_ID: &str = "space-decide-list";

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
    /// The app is installed at this place.
    Installed(PathBuf),
    /// The archive is unpacked into this folder.
    Unpacked(PathBuf),
    /// Same content as this file name, which stays.
    Duplicate(String),
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

/// One stop of the keyboard in the pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DecideKey {
    Head(DecideGroupKind),
    Item(PathBuf),
}

/// What the pane tells after a trash run or an undo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecideNotice {
    Trashed { count: usize, bytes: u64 },
    Undone { count: usize },
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
    RowPressed(PathBuf),
    RowToggled(PathBuf),
    HeadToggled(DecideGroupKind),
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
    pub(crate) folder: PathBuf,
    pub(crate) is_downloads: bool,
    pub(crate) groups: Vec<DecideGroup>,
    pub(crate) loaded: bool,
    pub(crate) chosen: HashSet<PathBuf>,
    pub(crate) folded: HashSet<DecideGroupKind>,
    pub(crate) cursor: Option<DecideKey>,
    pub(crate) empty_after: bool,
    pub(crate) notice: Option<DecideNotice>,
    pub(crate) can_undo: bool,
}

impl DecideState {
    pub(crate) fn new(folder: PathBuf, is_downloads: bool) -> Self {
        Self {
            folder,
            is_downloads,
            groups: Vec::new(),
            loaded: false,
            chosen: HashSet::new(),
            folded: HashSet::new(),
            cursor: None,
            empty_after: false,
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

    pub const fn empty_after(&self) -> bool {
        self.empty_after
    }

    pub const fn notice(&self) -> Option<DecideNotice> {
        self.notice
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

    /// The stops of the keyboard: a head per group and the items of the open ones.
    pub(crate) fn keys(&self) -> Vec<DecideKey> {
        let mut keys = Vec::new();
        for group in &self.groups {
            keys.push(DecideKey::Head(group.kind));
            if !self.is_folded(group.kind) {
                keys.extend(
                    group
                        .items
                        .iter()
                        .map(|entry| DecideKey::Item(entry.item.path.clone())),
                );
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
            DecideKey::Head(_) => None,
        }
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
            .fold(0, |sum, entry| sum.saturating_add(entry.item.size))
    }

    pub fn total_bytes(&self) -> u64 {
        self.groups
            .iter()
            .flat_map(|group| group.items.iter())
            .fold(0, |sum, entry| sum.saturating_add(entry.item.size))
    }

    /// How many groups have everything chosen, and how many groups there are.
    pub fn groups_chosen(&self) -> (usize, usize) {
        let chosen = self
            .groups
            .iter()
            .filter(|group| {
                !group.items.is_empty()
                    && group
                        .items
                        .iter()
                        .all(|entry| self.chosen.contains(&entry.item.path))
            })
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
