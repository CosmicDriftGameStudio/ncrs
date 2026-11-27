//! The rows the overview lists: one per entry of the scan, except the Downloads
//! folder, whose items are one row that opens the decide pane.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ncrs_space::{CatalogId, Entry, EntryKind, Group, Holder, Scan};

/// The groups in the order the overview shows them.
pub const GROUP_ORDER: [Group; 4] = [Group::Safe, Group::Worktrees, Group::Decide, Group::InUse];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowKind {
    Catalog(CatalogId),
    /// The branch of a merged worktree.
    Worktree(String),
    /// All the items of the Downloads folder.
    Downloads,
    BigFolder,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayRow {
    pub path: PathBuf,
    pub group: Group,
    pub kind: RowKind,
    pub bytes: u64,
    /// The places in `Scan::entries` the row stands for.
    pub entries: Vec<usize>,
    pub modified: Option<SystemTime>,
    pub held_by: Vec<Holder>,
    pub partial: bool,
}

impl DisplayRow {
    /// What a delete of the tab may remove: safe folders and merged worktrees.
    pub const fn is_deletable(&self) -> bool {
        matches!(self.group, Group::Safe | Group::Worktrees)
    }

    pub fn name(&self) -> String {
        match &self.kind {
            RowKind::Worktree(branch) => branch.clone(),
            RowKind::Catalog(_) | RowKind::Downloads | RowKind::BigFolder => {
                self.path.file_name().map_or_else(
                    || self.path.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                )
            }
        }
    }
}

/// One stop of the keyboard in the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Head(Group),
    /// The place in `State::rows`.
    Row(usize),
}

fn rank(group: Group) -> usize {
    GROUP_ORDER
        .iter()
        .position(|g| *g == group)
        .unwrap_or(GROUP_ORDER.len())
}

/// The rows of a scan, by group and then by size. In the Store build the worktrees and the
/// simulators are left out: ncrs cannot remove them there.
pub fn rows_of(scan: &Scan, home: &Path, store_build: bool) -> Vec<DisplayRow> {
    let downloads = home.join("Downloads");
    let mut rows: Vec<DisplayRow> = Vec::new();
    for (place, entry) in scan.entries.iter().enumerate() {
        if store_build && is_removed_in_store_build(entry) {
            continue;
        }
        if matches!(entry.kind, EntryKind::Decide(_)) && entry.path.starts_with(&downloads) {
            match rows.iter_mut().find(|row| row.kind == RowKind::Downloads) {
                Some(row) => {
                    row.bytes = row.bytes.saturating_add(entry.size.bytes);
                    row.modified = row.modified.max(entry.modified);
                    row.partial |= entry.size.partial;
                    row.entries.push(place);
                }
                None => rows.push(row_of(downloads.clone(), RowKind::Downloads, entry, place)),
            }
            continue;
        }
        let kind = match &entry.kind {
            EntryKind::Catalog(id) => RowKind::Catalog(*id),
            EntryKind::Worktree(worktree) => RowKind::Worktree(worktree.branch.clone()),
            EntryKind::Decide(_) => RowKind::BigFolder,
        };
        rows.push(row_of(entry.path.clone(), kind, entry, place));
    }
    rows.sort_by(|left, right| {
        rank(left.group)
            .cmp(&rank(right.group))
            .then(right.bytes.cmp(&left.bytes))
    });
    rows
}

fn is_removed_in_store_build(entry: &Entry) -> bool {
    matches!(
        entry.kind,
        EntryKind::Worktree(_) | EntryKind::Catalog(CatalogId::UnavailableSimulators)
    )
}

fn row_of(path: PathBuf, kind: RowKind, entry: &Entry, place: usize) -> DisplayRow {
    DisplayRow {
        path,
        group: entry.group,
        kind,
        bytes: entry.size.bytes,
        entries: vec![place],
        modified: entry.modified,
        held_by: entry.held_by.clone(),
        partial: entry.size.partial,
    }
}

/// The filter is a part of the name or of the path, in any case.
pub fn matches_filter(row: &DisplayRow, filter: &str) -> bool {
    let wanted = filter.trim().to_lowercase();
    wanted.is_empty()
        || row.name().to_lowercase().contains(&wanted)
        || row.path.to_string_lossy().to_lowercase().contains(&wanted)
}
