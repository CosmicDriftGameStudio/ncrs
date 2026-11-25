//! What the Git tab remembers.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ncrs_git::{
    ChangedFile, CommitDetail, FileDiff, Log, LogCommit, RefLabel, RefsOverview, WorktreeInfo,
};
use ncrs_ui_kit::git_rows::{CommitColumn, CommitColumns};

use crate::actions::{Actions, MenuTarget, WipFocus};
use crate::credentials::JobToken;
use crate::file_views::{self, BlameState, HistoryState};
use crate::panes::{PaneDrag, Split};
use crate::picker::Picker;

/// Commits asked for at once; the next page asks for this many more.
pub const PAGE: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Area {
    Sidebar,
    /// The bar between the sidebar and the commit list; the arrow keys move it.
    SidebarSplit,
    #[default]
    Commits,
    DetailSplit,
    Detail,
}

impl Area {
    /// The next stop of Tab, the splitters included.
    pub fn next(self) -> Self {
        match self {
            Self::Sidebar => Self::SidebarSplit,
            Self::SidebarSplit => Self::Commits,
            Self::Commits => Self::DetailSplit,
            Self::DetailSplit => Self::Detail,
            Self::Detail => Self::Sidebar,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::Sidebar => Self::Detail,
            Self::SidebarSplit => Self::Sidebar,
            Self::Commits => Self::SidebarSplit,
            Self::DetailSplit => Self::Commits,
            Self::Detail => Self::DetailSplit,
        }
    }

    /// The next list to the right, where the arrow keys go; the splitters are skipped.
    pub fn next_list(self) -> Self {
        match self {
            Self::Sidebar | Self::SidebarSplit => Self::Commits,
            Self::Commits | Self::DetailSplit | Self::Detail => Self::Detail,
        }
    }

    pub fn previous_list(self) -> Self {
        match self {
            Self::Sidebar | Self::SidebarSplit | Self::Commits => Self::Sidebar,
            Self::DetailSplit | Self::Detail => Self::Commits,
        }
    }

    /// The splitter this area is, if it is one.
    pub const fn split(self) -> Option<Split> {
        match self {
            Self::SidebarSplit => Some(Split::Sidebar),
            Self::DetailSplit => Some(Split::Detail),
            Self::Sidebar | Self::Commits | Self::Detail => None,
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
/// Remembered once the user has opened or shut "Remotes" themselves; until
/// then the group starts shut.
const REMOTES_SEEN_KEY: &str = "seen:remotes";

/// Whether the remembered folds say the user has chosen for "Remotes" already.
pub fn remotes_were_chosen(keys: &[String]) -> bool {
    keys.iter()
        .any(|key| key == REMOTES_SEEN_KEY || key == "remotes")
}

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
    /// The changes from one commit to another, or to the working copy (`None`).
    Compare {
        from: String,
        to: Option<String>,
    },
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
        let stop = Arc::new(AtomicBool::new(false));
        self.stop = Some(Arc::clone(&stop));
        (self.generation, stop)
    }

    /// Stops the read and drops its answer, also where the work could not be stopped.
    pub fn cancel(&mut self) {
        self.generation += 1;
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
    pub containing: Slot,
    pub history: Slot,
    pub history_diff: Slot,
    pub blame: Slot,
}

impl Loads {
    pub fn cancel_all(&mut self) {
        for slot in [
            &mut self.refs,
            &mut self.log,
            &mut self.detail,
            &mut self.diff,
            &mut self.actions,
            &mut self.containing,
            &mut self.history,
            &mut self.history_diff,
            &mut self.blame,
        ] {
            slot.cancel();
        }
    }
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
    /// A splitter being dragged.
    pub pane_drag: Option<PaneDrag>,
    /// The splitter the pointer is on.
    pub hovered_split: Option<Split>,
    /// The repository or branch picker that is open.
    pub picker: Option<Picker>,
    /// The user has opened or shut "Remotes" at least once.
    pub remotes_chosen: bool,
    /// The branches that contain the commit of the detail pane, with its id.
    pub containing: Option<(String, Vec<RefLabel>)>,
    /// The commit to go to as soon as the list has been read far enough.
    pub pending_jump: Option<String>,
    /// The id of the commit whose SHA was copied last, for the status line.
    pub copied_sha: Option<String>,
    /// The commit Space marked as the one to compare with.
    pub marked_commit: Option<String>,
    /// How far the user scrolled the commit list, in pixels.
    pub commits_scroll: f32,
    pub sidebar_cursor: usize,
    pub commit_cursor: usize,
    pub file_cursor: usize,
    pub tree_mode: bool,
    pub detail: Option<Detail>,
    pub diff: Option<DiffState>,
    /// The history of a file; a diff opened from it lies over it.
    pub history: Option<HistoryState>,
    /// The blame of a file; it lies over the diff and the history.
    pub blame: Option<BlameState>,
    /// Ready to show: why a read failed.
    pub problem: Option<String>,
    /// The actions of #338, filled by the caller while the design is open.
    pub actions: Actions,
    /// What the interactive jobs of this tab call themselves when git asks for a login.
    pub job_token: JobToken,
    pub loads: Loads,
}

impl State {
    pub fn is_open(&self) -> bool {
        self.root.is_some()
    }

    pub fn close(&mut self) {
        self.stop();
        // A login question of a job of this tab would wait for an answer nobody can give.
        crate::credentials::cancel_job(self.job_token);
        // The counters go on: an answer for the closed repository must not fit a new read.
        let loads = std::mem::take(&mut self.loads);
        *self = Self {
            loads,
            ..Self::default()
        };
    }

    /// Stops every read in flight, at quit.
    pub fn stop(&mut self) {
        self.loads.cancel_all();
    }

    /// Closes what lies over the commit list: the blame, the diff and the history.
    pub fn close_layers(&mut self) {
        self.blame = None;
        self.diff = None;
        self.history = None;
        self.loads.blame.cancel();
        self.loads.diff.cancel();
        self.loads.history.cancel();
        self.loads.history_diff.cancel();
    }

    /// Something lies over the commit list: its keys must not act on rows nobody sees.
    pub fn list_is_covered(&self) -> bool {
        self.blame.is_some() || self.diff.is_some() || self.history.is_some()
    }

    /// The diff lists its files itself: the detail pane is about something else.
    pub fn diff_has_own_list(&self) -> bool {
        self.diff.as_ref().is_some_and(|shown| {
            self.detail
                .as_ref()
                .is_none_or(|detail| detail.subject != shown.subject)
        })
    }

    /// A diff has the keys of a diff.
    fn diff_has_keys(&self) -> bool {
        self.blame.is_none() && (self.diff.is_some() || self.wip_diff_has_keys())
    }

    /// The diff of a local change is open and no commit field has the keys.
    fn wip_diff_has_keys(&self) -> bool {
        self.changes_shown()
            && self.actions.local.as_ref().is_some_and(|local| {
                local.diff_open && matches!(local.focus, WipFocus::Unstaged | WipFocus::Staged)
            })
    }

    /// What the keys of the tab need to know.
    pub fn key_context(&self) -> crate::keys::KeyContext {
        crate::keys::KeyContext {
            filter_focused: self.filter_focused,
            in_diff: self.diff_has_keys(),
            list_covered: self.list_is_covered(),
            dialog_open: self.actions.dialog_open(),
            cleanup_open: matches!(self.actions.dialog, Some(crate::actions::Dialog::Cleanup(_))),
            picker_open: self.picker.is_some(),
            sync_menu_open: self.actions.sync_menu.is_some(),
            area: self.area,
        }
    }

    pub fn sidebar_items(&self) -> Vec<SidebarItem> {
        let Some(overview) = &self.overview else {
            return Vec::new();
        };
        let is_folded = |section: Section| self.collapsed.contains(&Fold::Section(section));
        let mut items = Vec::new();
        items.push(SidebarItem::Header(Section::Local));
        if !is_folded(Section::Local) {
            items.extend((0..overview.local.len()).map(SidebarItem::Local));
        }
        items.push(SidebarItem::Header(Section::Worktrees));
        if !is_folded(Section::Worktrees) {
            items.extend((0..overview.worktrees.len()).map(SidebarItem::Worktree));
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
        if self.remotes_chosen {
            keys.push(REMOTES_SEEN_KEY.to_owned());
        }
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

    /// How many files are changed: the lists the pane shows when there are
    /// any, else what the overview of the worktree counted.
    pub fn local_change_count(&self) -> usize {
        match &self.actions.local {
            Some(local) => local.unstaged.len() + local.staged.len(),
            None => self.current_worktree().map_or(0, |worktree| worktree.wip),
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

    /// The first row of the list stands for the changes of the working copy:
    /// with the actions on, the files the last read of git found; else the
    /// count of the overview.
    pub fn has_working_row(&self) -> bool {
        match &self.actions.local {
            Some(local) => !local.unstaged.is_empty() || !local.staged.is_empty(),
            None => self
                .current_worktree()
                .is_some_and(|worktree| worktree.wip > 0),
        }
    }

    /// The cursor of the list is on the working copy row.
    pub fn working_row_chosen(&self) -> bool {
        self.has_working_row() && self.commit_cursor == 0
    }

    /// The right side shows the staging of the local changes: the row of the
    /// working copy is the chosen one, git can write and no diff or history of a
    /// file took its place.
    pub fn changes_shown(&self) -> bool {
        self.actions.local.is_some()
            && self.working_row_chosen()
            && self.diff.is_none()
            && self.history.is_none()
    }

    pub fn row_count(&self) -> usize {
        usize::from(self.has_working_row()) + self.log.as_ref().map_or(0, |log| log.commits.len())
    }

    /// The commit of a row of the list; `None` for the working copy row.
    pub fn commit_at(&self, row: usize) -> Option<&LogCommit> {
        let index = row.checked_sub(usize::from(self.has_working_row()))?;
        self.log.as_ref()?.commits.get(index)
    }

    /// The row of the list that shows the commit, the working copy row counted.
    pub fn row_of_commit(&self, sha: &str) -> Option<usize> {
        let index = self
            .log
            .as_ref()?
            .commits
            .iter()
            .position(|commit| commit.id == sha)?;
        Some(index + usize::from(self.has_working_row()))
    }

    /// Closes the open menu when the commit, stash or file it was opened on is gone.
    pub fn close_menu_if_stale(&mut self) {
        let gone = match self.actions.menu.as_ref().map(|open| &open.target) {
            Some(MenuTarget::Commit(sha)) => self.row_of_commit(sha).is_none(),
            Some(MenuTarget::Stash(id)) => !self.actions.stash_ids.contains(id),
            Some(MenuTarget::Conflict(path)) => !self
                .actions
                .merge
                .as_ref()
                .is_some_and(|merge| merge.conflicts.iter().any(|it| it.path == *path)),
            Some(MenuTarget::File { path, .. }) => file_views::row_of_file(self, path).is_none(),
            Some(MenuTarget::LocalBranch(_) | MenuTarget::RemoteBranch(_) | MenuTarget::Worktree(_)) | None => false,
        };
        if gone {
            self.actions.menu = None;
        }
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
