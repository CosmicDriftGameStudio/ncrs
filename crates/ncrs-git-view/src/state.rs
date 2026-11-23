//! What the Git tab remembers.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ncrs_git::{ChangedFile, CommitDetail, FileDiff, Log, LogCommit, RefsOverview, WorktreeInfo};
use ncrs_ui_kit::git_rows::{CommitColumn, CommitColumns};

use crate::actions::Actions;

/// Commits asked for at once; the next page asks for this many more.
pub const PAGE: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Area {
    Sidebar,
    #[default]
    Commits,
    Detail,
}

impl Area {
    pub fn next(self) -> Self {
        match self {
            Self::Sidebar => Self::Commits,
            Self::Commits => Self::Detail,
            Self::Detail => Self::Sidebar,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::Sidebar => Self::Detail,
            Self::Commits => Self::Sidebar,
            Self::Detail => Self::Commits,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Local,
    Remotes,
    Tags,
    Stashes,
    Worktrees,
}

/// Something in the left list that folds: a group, or one remote below "Remotes".
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Fold {
    Section(Section),
    Remote(String),
}

const REMOTE_KEY_PREFIX: &str = "remote:";

impl Fold {
    /// How the app remembers it.
    pub fn key(&self) -> String {
        match self {
            Self::Section(Section::Local) => "local".to_owned(),
            Self::Section(Section::Remotes) => "remotes".to_owned(),
            Self::Section(Section::Tags) => "tags".to_owned(),
            Self::Section(Section::Stashes) => "stashes".to_owned(),
            Self::Section(Section::Worktrees) => "worktrees".to_owned(),
            Self::Remote(name) => format!("{REMOTE_KEY_PREFIX}{name}"),
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        let section = match key {
            "local" => Section::Local,
            "remotes" => Section::Remotes,
            "tags" => Section::Tags,
            "stashes" => Section::Stashes,
            "worktrees" => Section::Worktrees,
            _ => {
                return key
                    .strip_prefix(REMOTE_KEY_PREFIX)
                    .map(|name| Self::Remote(name.to_owned()))
            }
        };
        Some(Self::Section(section))
    }
}

/// One remote with its branches, as the left list shows them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteGroup<'a> {
    pub name: &'a str,
    /// The index into [`RefsOverview::remote`] and the branch without `<remote>/`.
    pub branches: Vec<(usize, &'a str)>,
}

/// The remote branches grouped by the remote they come from, in the order the
/// remotes first appear.
pub fn remote_groups(overview: &RefsOverview) -> Vec<RemoteGroup<'_>> {
    let mut groups: Vec<RemoteGroup<'_>> = Vec::new();
    for (index, full) in overview.remote.iter().enumerate() {
        let (name, branch) = full.split_once('/').unwrap_or((full, full));
        match groups.iter_mut().find(|group| group.name == name) {
            Some(group) => group.branches.push((index, branch)),
            None => groups.push(RemoteGroup {
                name,
                branches: vec![(index, branch)],
            }),
        }
    }
    groups
}

/// One line of the sidebar; the indexes point into the [`RefsOverview`], the
/// remote header into [`remote_groups`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarItem {
    /// The local changes, above the groups; only while there are actions to show them.
    Changes,
    Header(Section),
    Local(usize),
    RemoteHeader(usize),
    Remote(usize),
    Tag(usize),
    Worktree(usize),
    Stash(usize),
}

/// A commit column whose width is being dragged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColumnDrag {
    pub column: CommitColumn,
    /// The widths when the press began.
    pub start: CommitColumns,
    /// Where the pointer was at the first move after the press.
    pub origin: Option<f32>,
    /// The widths as dragged so far.
    pub current: CommitColumns,
}

/// What the right side of the Git tab is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    /// The changes in the working copy against HEAD.
    Working,
    Commit(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Detail {
    pub subject: Subject,
    pub info: Option<CommitDetail>,
    pub files: Vec<ChangedFile>,
}

/// A line of the file list of the detail pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailRow {
    Folder {
        depth: usize,
        name: String,
    },
    File {
        depth: usize,
        name: String,
        index: usize,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiffState {
    pub subject: Subject,
    pub files: Vec<ChangedFile>,
    pub file: usize,
    pub diff: Option<FileDiff>,
    pub unified: bool,
    pub ignore_whitespace: bool,
    pub hunk: usize,
}

/// A blocking read in flight: a newer one stops the older and its answer is dropped.
#[derive(Debug, Default)]
pub struct Slot {
    generation: u64,
    stop: Option<Arc<AtomicBool>>,
}

impl Slot {
    pub fn begin(&mut self) -> (u64, Arc<AtomicBool>) {
        self.cancel();
        self.generation += 1;
        let stop = Arc::new(AtomicBool::new(false));
        self.stop = Some(Arc::clone(&stop));
        (self.generation, stop)
    }

    pub fn cancel(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
    }

    pub fn is_current(&self, generation: u64) -> bool {
        self.generation == generation
    }
}

#[derive(Debug, Default)]
pub struct Loads {
    pub refs: Slot,
    pub log: Slot,
    pub detail: Slot,
    pub diff: Slot,
    pub actions: Slot,
}

#[derive(Debug, Default)]
pub struct State {
    pub root: Option<PathBuf>,
    pub name: String,
    pub overview: Option<RefsOverview>,
    pub log: Option<Log>,
    pub limit: usize,
    pub filter: String,
    pub filter_focused: bool,
    pub area: Area,
    pub collapsed: HashSet<Fold>,
    /// A commit column being dragged to another width.
    pub column_drag: Option<ColumnDrag>,
    pub sidebar_cursor: usize,
    pub commit_cursor: usize,
    pub file_cursor: usize,
    pub tree_mode: bool,
    pub detail: Option<Detail>,
    pub diff: Option<DiffState>,
    /// Ready to show: why a read failed.
    pub problem: Option<String>,
    /// The actions of #338, filled by the caller while the design is open.
    pub actions: Actions,
    pub loads: Loads,
}

impl State {
    pub fn is_open(&self) -> bool {
        self.root.is_some()
    }

    pub fn close(&mut self) {
        self.loads.refs.cancel();
        self.loads.log.cancel();
        self.loads.detail.cancel();
        self.loads.diff.cancel();
        self.loads.actions.cancel();
        *self = Self::default();
    }

    /// Stops every read in flight, at quit.
    pub fn stop(&mut self) {
        self.loads.refs.cancel();
        self.loads.log.cancel();
        self.loads.detail.cancel();
        self.loads.diff.cancel();
        self.loads.actions.cancel();
    }

    pub fn sidebar_items(&self) -> Vec<SidebarItem> {
        let Some(overview) = &self.overview else {
            return Vec::new();
        };
        let is_folded = |section: Section| self.collapsed.contains(&Fold::Section(section));
        let mut items = Vec::new();
        if self.actions.local.is_some() {
            items.push(SidebarItem::Changes);
        }
        items.push(SidebarItem::Header(Section::Local));
        if !is_folded(Section::Local) {
            items.extend((0..overview.local.len()).map(SidebarItem::Local));
        }
        items.push(SidebarItem::Header(Section::Remotes));
        if !is_folded(Section::Remotes) {
            for (position, group) in remote_groups(overview).iter().enumerate() {
                items.push(SidebarItem::RemoteHeader(position));
                if !self
                    .collapsed
                    .contains(&Fold::Remote(group.name.to_owned()))
                {
                    items.extend(
                        group
                            .branches
                            .iter()
                            .map(|(index, _)| SidebarItem::Remote(*index)),
                    );
                }
            }
        }
        items.push(SidebarItem::Header(Section::Tags));
        if !is_folded(Section::Tags) {
            items.extend((0..overview.tags.len()).map(SidebarItem::Tag));
        }
        items.push(SidebarItem::Header(Section::Stashes));
        if !is_folded(Section::Stashes) {
            items.extend((0..self.actions.stashes.len()).map(SidebarItem::Stash));
        }
        items.push(SidebarItem::Header(Section::Worktrees));
        if !is_folded(Section::Worktrees) {
            items.extend((0..overview.worktrees.len()).map(SidebarItem::Worktree));
        }
        items
    }

    /// What a press on `item` folds or unfolds; `None` for a row that is no group.
    pub fn fold_of(&self, item: SidebarItem) -> Option<Fold> {
        match item {
            SidebarItem::Header(section) => Some(Fold::Section(section)),
            SidebarItem::RemoteHeader(position) => {
                let overview = self.overview.as_ref()?;
                let groups = remote_groups(overview);
                let group = groups.get(position)?;
                Some(Fold::Remote(group.name.to_owned()))
            }
            _ => None,
        }
    }

    /// The folded groups as the app remembers them, in a fixed order.
    pub fn folded_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.collapsed.iter().map(Fold::key).collect();
        keys.sort();
        keys
    }

    pub fn section_count(&self, section: Section) -> usize {
        let Some(overview) = &self.overview else {
            return 0;
        };
        match section {
            Section::Local => overview.local.len(),
            Section::Remotes => overview.remote.len(),
            Section::Tags => overview.tags.len(),
            Section::Stashes => overview.stashes.max(self.actions.stashes.len()),
            Section::Worktrees => overview.worktrees.len(),
        }
    }

    /// The worktree whose folder this tab was opened on.
    pub fn current_worktree(&self) -> Option<&WorktreeInfo> {
        self.overview
            .as_ref()?
            .worktrees
            .iter()
            .find(|worktree| worktree.is_current)
    }

    /// The commit columns as drawn: the ones being dragged, else the remembered ones.
    pub fn columns_in_view(&self, remembered: CommitColumns) -> CommitColumns {
        self.column_drag.map_or(remembered, |drag| drag.current)
    }

    /// The first row of the list stands for the changes of the working copy.
    pub fn has_working_row(&self) -> bool {
        self.actions.local.is_none()
            && self
                .current_worktree()
                .is_some_and(|worktree| worktree.wip > 0)
    }

    pub fn row_count(&self) -> usize {
        usize::from(self.has_working_row()) + self.log.as_ref().map_or(0, |log| log.commits.len())
    }

    /// The commit of a row of the list; `None` for the working copy row.
    pub fn commit_at(&self, row: usize) -> Option<&LogCommit> {
        let index = row.checked_sub(usize::from(self.has_working_row()))?;
        self.log.as_ref()?.commits.get(index)
    }

    pub fn subject_at(&self, row: usize) -> Option<Subject> {
        if row == 0 && self.has_working_row() {
            return Some(Subject::Working);
        }
        self.commit_at(row)
            .map(|commit| Subject::Commit(commit.id.clone()))
    }

    pub fn files_in_detail(&self) -> &[ChangedFile] {
        self.detail
            .as_ref()
            .map_or(&[], |detail| detail.files.as_slice())
    }

    /// The file list of the detail pane: every path, or a tree of folders.
    pub fn detail_rows(&self) -> Vec<DetailRow> {
        let files = self.files_in_detail();
        if !self.tree_mode {
            return files
                .iter()
                .enumerate()
                .map(|(index, file)| DetailRow::File {
                    depth: 0,
                    name: file.path.clone(),
                    index,
                })
                .collect();
        }
        let mut rows = Vec::new();
        let mut open: Vec<&str> = Vec::new();
        for (index, file) in files.iter().enumerate() {
            let parts: Vec<&str> = file.path.split('/').collect();
            let (name, folders) = match parts.split_last() {
                Some((name, folders)) => (*name, folders),
                None => continue,
            };
            let shared = open
                .iter()
                .zip(folders)
                .take_while(|(left, right)| left == right)
                .count();
            open.truncate(shared);
            for (depth, folder) in folders.iter().enumerate().skip(shared) {
                rows.push(DetailRow::Folder {
                    depth,
                    name: (*folder).to_owned(),
                });
                open.push(*folder);
            }
            rows.push(DetailRow::File {
                depth: folders.len(),
                name: name.to_owned(),
                index,
            });
        }
        rows
    }
}
