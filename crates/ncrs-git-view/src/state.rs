//! What the Git tab remembers.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ncrs_git::{ChangedFile, CommitDetail, FileDiff, Log, LogCommit, RefsOverview, WorktreeInfo};

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
    Remote,
    Tags,
    Stashes,
    Worktrees,
}

/// One line of the sidebar; the indexes point into the [`RefsOverview`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarItem {
    Header(Section),
    Local(usize),
    Remote(usize),
    Tag(usize),
    Worktree(usize),
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
    pub collapsed: HashSet<Section>,
    pub sidebar_cursor: usize,
    pub commit_cursor: usize,
    pub file_cursor: usize,
    pub tree_mode: bool,
    pub detail: Option<Detail>,
    pub diff: Option<DiffState>,
    /// Ready to show: why a read failed.
    pub problem: Option<String>,
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
        *self = Self::default();
    }

    /// Stops every read in flight, at quit.
    pub fn stop(&mut self) {
        self.loads.refs.cancel();
        self.loads.log.cancel();
        self.loads.detail.cancel();
        self.loads.diff.cancel();
    }

    pub fn sidebar_items(&self) -> Vec<SidebarItem> {
        let Some(overview) = &self.overview else {
            return Vec::new();
        };
        let mut items = Vec::new();
        let mut section = |section: Section, count: usize, item: fn(usize) -> SidebarItem| {
            items.push(SidebarItem::Header(section));
            if !self.collapsed.contains(&section) {
                items.extend((0..count).map(item));
            }
        };
        section(Section::Local, overview.local.len(), SidebarItem::Local);
        section(Section::Remote, overview.remote.len(), SidebarItem::Remote);
        section(Section::Tags, overview.tags.len(), SidebarItem::Tag);
        section(Section::Stashes, 0, SidebarItem::Tag);
        section(
            Section::Worktrees,
            overview.worktrees.len(),
            SidebarItem::Worktree,
        );
        items
    }

    pub fn section_count(&self, section: Section) -> usize {
        let Some(overview) = &self.overview else {
            return 0;
        };
        match section {
            Section::Local => overview.local.len(),
            Section::Remote => overview.remote.len(),
            Section::Tags => overview.tags.len(),
            Section::Stashes => overview.stashes,
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

    /// The first row of the list stands for the changes of the working copy.
    pub fn has_working_row(&self) -> bool {
        self.current_worktree()
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
