use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use ncrs_ui_kit::git_state::RunOutcome;
use ncrs_ui_kit::marked_list::MarkedList;
use ncrs_ui_kit::repo_row::RunSummary;

use crate::bulk::{BulkAction, Outcome};
use crate::log::Log;
use crate::rows::{self, Group, GroupKey, Row, RowKind, Shape};
use crate::store::Store;
use crate::summary::Reading;

/// What a bulk action applies to (G17): the marked repos, else the favourites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Marked(usize),
    Favourites(usize),
}

impl Scope {
    pub const fn count(self) -> usize {
        match self {
            Self::Marked(count) | Self::Favourites(count) => count,
        }
    }
}

/// A bulk action that has been started, and what came of it.
#[derive(Debug)]
pub struct Run {
    pub action: BulkAction,
    pub scope: Scope,
    /// Every repository of the run, in the order it was started.
    pub order: Vec<PathBuf>,
    pub results: BTreeMap<PathBuf, Outcome>,
    pub queue: VecDeque<PathBuf>,
    pub running: usize,
    pub only_problems: bool,
    /// The log shows only this repository.
    pub details: Option<PathBuf>,
    pub log: Log,
}

impl Run {
    pub const fn action(&self) -> BulkAction {
        self.action
    }

    pub const fn scope(&self) -> Scope {
        self.scope
    }

    pub fn targets(&self) -> &[PathBuf] {
        &self.order
    }

    pub fn result(&self, path: &Path) -> Option<&Outcome> {
        self.results.get(path)
    }

    /// How many commands run right now.
    pub const fn running(&self) -> usize {
        self.running
    }

    /// How many repositories wait for a free place.
    pub fn waiting(&self) -> usize {
        self.queue.len()
    }

    pub fn is_finished(&self) -> bool {
        self.running == 0 && self.queue.is_empty()
    }

    pub const fn only_problems(&self) -> bool {
        self.only_problems
    }

    pub fn log(&self) -> &Log {
        &self.log
    }

    pub fn details(&self) -> Option<&Path> {
        self.details.as_deref()
    }

    /// Repositories that have an end result.
    pub fn done(&self) -> usize {
        self.results
            .values()
            .filter(|outcome| outcome.kind() != RunOutcome::Running)
            .count()
    }

    pub fn summary(&self) -> RunSummary {
        let kinds: Vec<RunOutcome> = self.results.values().map(Outcome::kind).collect();
        RunSummary::of(&kinds)
    }

    pub fn failed(&self) -> Vec<PathBuf> {
        self.order
            .iter()
            .filter(|path| self.results.get(*path).is_some_and(Outcome::is_failure))
            .cloned()
            .collect()
    }
}

/// The tab.
#[derive(Debug)]
pub struct State {
    pub store: Store,
    pub file: Option<PathBuf>,
    pub readings: BTreeMap<PathBuf, Reading>,
    pub marks: MarkedList<PathBuf>,
    pub filter: String,
    pub filter_focused: bool,
    pub only_needing_action: bool,
    pub folded: BTreeSet<GroupKey>,
    pub hovered: Option<PathBuf>,
    pub open_roots: Vec<PathBuf>,
    /// The flag of the scan that runs; none when none does.
    pub scanning: Option<Arc<AtomicBool>>,
    pub read_queue: VecDeque<PathBuf>,
    pub reads_running: usize,
    pub run: Option<Run>,
    /// The menu is open for this row; for the bar when `None` inside.
    pub menu: Option<PathBuf>,
    pub failure: Option<String>,
    pub clone: Option<crate::clone::CloneDraft>,
    pub stop: Arc<AtomicBool>,
}

impl State {
    pub fn new(store: Store, file: Option<PathBuf>) -> Self {
        Self {
            store,
            file,
            readings: BTreeMap::new(),
            marks: MarkedList::new(),
            filter: String::new(),
            filter_focused: false,
            only_needing_action: false,
            folded: BTreeSet::new(),
            hovered: None,
            open_roots: Vec::new(),
            scanning: None,
            read_queue: VecDeque::new(),
            reads_running: 0,
            run: None,
            menu: None,
            failure: None,
            clone: None,
            stop: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn reading(&self, path: &Path) -> Option<&Reading> {
        self.readings.get(path)
    }

    pub fn run(&self) -> Option<&Run> {
        self.run.as_ref()
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub const fn filter_focused(&self) -> bool {
        self.filter_focused
    }

    pub const fn only_needing_action(&self) -> bool {
        self.only_needing_action
    }

    pub const fn is_scanning(&self) -> bool {
        self.scanning.is_some()
    }

    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    pub const fn menu_open(&self) -> bool {
        self.menu.is_some()
    }

    pub fn hovered(&self) -> Option<&Path> {
        self.hovered.as_deref()
    }

    /// Whether the tab holds the keys of its own: the filter or the clone
    /// dialog has the keyboard.
    pub const fn takes_text(&self) -> bool {
        self.clone.is_some() || self.filter_focused
    }

    pub const fn clone_draft(&self) -> Option<&crate::clone::CloneDraft> {
        self.clone.as_ref()
    }

    pub fn is_folded(&self, key: &GroupKey) -> bool {
        self.folded.contains(key)
    }

    /// The groups as shown: with a run, only the repos it ran on, and with
    /// "only problems" only those that went wrong.
    pub fn groups(&self) -> Vec<Group> {
        let unfolded = BTreeSet::new();
        // A run shows what it ran on, every group open.
        let folded = if self.run.is_some() {
            &unfolded
        } else {
            &self.folded
        };
        let shape = Shape {
            readings: &self.readings,
            open_roots: &self.open_roots,
            filter: &self.filter,
            only_needing_action: self.only_needing_action,
            folded,
        };
        let mut groups = rows::build(&self.store, &shape);
        if let Some(run) = &self.run {
            let keeps = |path: &Path| {
                run.order.iter().any(|target| target == path)
                    && (!run.only_problems
                        || run.results.get(path).is_some_and(Outcome::is_problem))
            };
            for group in &mut groups {
                group.rows.retain(|row| keeps(&row.owner));
                group.count = group
                    .rows
                    .iter()
                    .filter(|row| row.kind == RowKind::Repo)
                    .count();
            }
            groups.retain(|group| group.count > 0);
        }
        groups
    }

    /// Every row the cursor can stand on, top to bottom.
    pub fn visible(&self) -> Vec<PathBuf> {
        rows::visible_paths(&self.groups())
    }

    pub fn cursor(&self) -> Option<PathBuf> {
        self.marks.cursor(&self.visible()).cloned()
    }

    pub fn is_marked(&self, path: &Path) -> bool {
        self.marks.is_marked(&path.to_path_buf())
    }

    /// The marked repositories, worktrees left out, in path order.
    pub fn marked_repos(&self) -> Vec<PathBuf> {
        self.marks
            .marked()
            .filter(|path| self.store.contains(path))
            .cloned()
            .collect()
    }

    /// What a bulk action would run on: the marked repos, else the favourites.
    pub fn targets(&self) -> (Scope, Vec<PathBuf>) {
        let marked = self.marked_repos();
        if marked.is_empty() {
            let favourites = self.store.favourites();
            (Scope::Favourites(favourites.len()), favourites)
        } else {
            (Scope::Marked(marked.len()), marked)
        }
    }

    pub fn row(&self, path: &Path) -> Option<Row> {
        self.groups()
            .into_iter()
            .flat_map(|group| group.rows)
            .find(|row| row.path == path)
    }

    /// Repos behind their remote, repos with local changes, for the summary line.
    pub fn counts(&self) -> (usize, usize, usize) {
        let summaries = self.readings.values().filter_map(Reading::summary);
        let (mut changed, mut behind) = (0, 0);
        for summary in summaries {
            changed += usize::from(summary.changed > 0);
            behind += usize::from(summary.behind > 0);
        }
        (self.store.repos.len(), changed, behind)
    }
}
