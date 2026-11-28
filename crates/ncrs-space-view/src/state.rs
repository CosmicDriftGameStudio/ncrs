use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ncrs_space::{Group, LoggedRun, RunLog, Scan, UnknownUse};

use crate::confirm::{ConfirmKind, ConfirmState, RunPlan};
use crate::decide::DecideState;
use crate::rows::{matches_filter, rows_of, DisplayRow, Item, GROUP_ORDER};

/// A measure that runs: the flag stops it, `bytes` is what it has counted so far.
#[derive(Debug, Clone)]
pub struct Measuring {
    pub run: u64,
    pub stop: Arc<AtomicBool>,
    pub bytes: u64,
}

/// A delete run that goes on.
#[derive(Debug, Clone)]
pub struct Running {
    pub run: u64,
    pub stop: Arc<AtomicBool>,
    pub plan: RunPlan,
    pub kind: ConfirmKind,
    pub done: usize,
    pub total: usize,
    pub free_before: u64,
}

/// The merged worktrees listed before the rest folds into one row.
pub const WORKTREES_SHOWN: usize = 8;

/// How the last run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultState {
    pub log: Arc<RunLog>,
    pub kind: ConfirmKind,
    pub unknown: UnknownUse,
    pub free_before: u64,
    pub free_after: Option<u64>,
    /// This run as the log file has it, with the time of every row, once read.
    pub log_run: Option<LoggedRun>,
}

impl ResultState {
    pub fn freed(&self) -> u64 {
        self.log.freed_bytes()
    }

    pub fn deleted(&self) -> usize {
        self.log
            .rows
            .iter()
            .filter(|row| row.outcome.is_ok())
            .count()
    }

    pub fn skipped(&self) -> usize {
        self.log
            .rows
            .iter()
            .filter(|row| row.outcome.is_err())
            .count()
    }

    /// What the run did to the folder at `path`.
    pub fn logged(&self, path: &Path) -> Option<&ncrs_space::LogRow> {
        self.log.rows.iter().find(|row| row.path == path)
    }

    pub fn is_deleted(&self, path: &Path) -> bool {
        self.logged(path).is_some_and(|row| row.outcome.is_ok())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CursorKey {
    Head(Group),
    Row(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MenuState {
    pub path: PathBuf,
    pub cursor: usize,
}

#[derive(Debug, Default)]
pub struct State {
    pub(crate) scan: Option<Arc<Scan>>,
    pub(crate) rows: Vec<DisplayRow>,
    pub(crate) scan_run: u64,
    pub(crate) run_id: u64,
    pub(crate) measuring: Option<Measuring>,
    pub(crate) folded: HashSet<Group>,
    pub(crate) unticked: HashSet<PathBuf>,
    pub(crate) cursor: Option<CursorKey>,
    pub(crate) filter: String,
    pub(crate) filter_focused: bool,
    pub(crate) menu: Option<MenuState>,
    pub(crate) confirm: Option<ConfirmState>,
    pub(crate) running: Option<Running>,
    pub(crate) result: Option<ResultState>,
    pub(crate) failure: Option<String>,
    pub(crate) list_scroll: f32,
    pub(crate) decide: Option<DecideState>,
    pub(crate) show_rules: bool,
    /// All merged worktrees are listed, not only the first ones.
    pub(crate) worktrees_expanded: bool,
    /// A measure that ended while the result was up: the rows follow when it closes, so
    /// the deleted ones stay in the list as the run left them.
    pub(crate) pending_scan: Option<Arc<Scan>>,
}

impl State {
    pub fn scan(&self) -> Option<&Arc<Scan>> {
        self.scan.as_ref()
    }

    /// The drive as it is now: after a run the newer measure, else the one of the list.
    /// Drive line, chips, status bar and result banner all read this one place.
    pub fn volume_now(&self) -> Option<ncrs_space::VolumeSpace> {
        let scan = self.pending_scan.as_ref().or(self.scan.as_ref())?;
        Some(scan.volume)
    }

    /// The number of the measure that runs or ran last; its answers carry it.
    pub const fn scan_run(&self) -> u64 {
        self.scan_run
    }

    /// The number of the delete run that runs or ran last.
    pub const fn run_id(&self) -> u64 {
        self.run_id
    }

    pub const fn measuring(&self) -> Option<&Measuring> {
        self.measuring.as_ref()
    }

    pub const fn is_measuring(&self) -> bool {
        self.measuring.is_some()
    }

    pub const fn confirm(&self) -> Option<&ConfirmState> {
        self.confirm.as_ref()
    }

    pub const fn running(&self) -> Option<&Running> {
        self.running.as_ref()
    }

    pub const fn result(&self) -> Option<&ResultState> {
        self.result.as_ref()
    }

    /// How many rows of the group the list shows: the merged worktrees fold after the first
    /// ones until the user opens them, or filters.
    pub fn row_limit(&self, group: Group) -> usize {
        if group == Group::Worktrees && !self.worktrees_expanded && self.filter.is_empty() {
            WORKTREES_SHOWN
        } else {
            usize::MAX
        }
    }

    /// The newer measure that waits for the result to close.
    pub fn pending_scan(&self) -> Option<&Scan> {
        self.pending_scan.as_deref()
    }

    pub const fn show_rules(&self) -> bool {
        self.show_rules
    }

    pub const fn decide(&self) -> Option<&DecideState> {
        self.decide.as_ref()
    }

    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub const fn filter_focused(&self) -> bool {
        self.filter_focused
    }

    pub const fn menu_open(&self) -> bool {
        self.menu.is_some()
    }

    pub fn menu_path(&self) -> Option<&Path> {
        self.menu.as_ref().map(|menu| menu.path.as_path())
    }

    pub fn menu_cursor(&self) -> usize {
        self.menu.as_ref().map_or(0, |menu| menu.cursor)
    }

    pub fn rows(&self) -> &[DisplayRow] {
        &self.rows
    }

    pub fn is_folded(&self, group: Group) -> bool {
        self.folded.contains(&group)
    }

    /// Whether the row is in the next delete: a deletable row is, unless it was unticked.
    pub fn is_ticked(&self, row: &DisplayRow) -> bool {
        row.is_deletable() && !self.unticked.contains(&row.path)
    }

    /// The stops of the keyboard: the heads of the groups with rows, and the rows of the open
    /// groups that the filter lets through.
    pub fn items(&self) -> Vec<Item> {
        let mut items = Vec::new();
        for group in GROUP_ORDER {
            let rows: Vec<usize> = self
                .rows
                .iter()
                .enumerate()
                .filter(|(_, row)| row.group == group && matches_filter(row, &self.filter))
                .map(|(place, _)| place)
                .collect();
            if rows.is_empty() {
                continue;
            }
            items.push(Item::Head(group));
            let listed_apart = self.result.is_some() && group == Group::Worktrees;
            if !self.is_folded(group) && !listed_apart {
                items.extend(rows.into_iter().take(self.row_limit(group)).map(Item::Row));
            }
        }
        items
    }

    pub fn cursor_index(&self) -> Option<usize> {
        let key = self.cursor.as_ref()?;
        self.items()
            .iter()
            .position(|item| self.key_of(*item) == *key)
    }

    pub fn cursor_item(&self) -> Option<Item> {
        self.items().get(self.cursor_index()?).copied()
    }

    /// The row under the cursor; none while the cursor is on a head.
    pub fn cursor_row(&self) -> Option<&DisplayRow> {
        match self.cursor_item()? {
            Item::Row(place) => self.rows.get(place),
            Item::Head(_) => None,
        }
    }

    pub fn cursor_path(&self) -> Option<&Path> {
        self.cursor_row().map(|row| row.path.as_path())
    }

    pub(crate) fn key_of(&self, item: Item) -> CursorKey {
        match item {
            Item::Head(group) => CursorKey::Head(group),
            Item::Row(place) => self
                .rows
                .get(place)
                .map_or(CursorKey::Head(Group::Safe), |row| {
                    CursorKey::Row(row.path.clone())
                }),
        }
    }

    pub(crate) fn set_cursor_index(&mut self, index: usize) {
        let items = self.items();
        if let Some(item) = items.get(index.min(items.len().saturating_sub(1))) {
            self.cursor = Some(self.key_of(*item));
        }
    }

    pub(crate) fn row_by_path(&self, path: &Path) -> Option<&DisplayRow> {
        self.rows.iter().find(|row| row.path == path)
    }

    pub fn group_rows(&self, group: Group) -> impl Iterator<Item = &DisplayRow> {
        self.rows.iter().filter(move |row| row.group == group)
    }

    pub fn group_bytes(&self, group: Group) -> u64 {
        self.group_rows(group)
            .fold(0, |sum, row| sum.saturating_add(row.bytes))
    }

    pub fn ticked_rows(&self, group: Group) -> impl Iterator<Item = &DisplayRow> {
        self.group_rows(group).filter(|row| self.is_ticked(row))
    }

    pub fn ticked_bytes(&self, group: Group) -> u64 {
        self.ticked_rows(group)
            .fold(0, |sum, row| sum.saturating_add(row.bytes))
    }

    pub fn ticked_count(&self, group: Group) -> usize {
        self.ticked_rows(group).count()
    }

    /// Whether the last run already deleted the row.
    pub fn is_done(&self, row: &DisplayRow) -> bool {
        self.result
            .as_ref()
            .is_some_and(|result| result.is_deleted(&row.path))
    }

    /// The ticked rows of the group that the last run did not delete already.
    pub fn open_ticked_rows(&self, group: Group) -> impl Iterator<Item = &DisplayRow> {
        self.ticked_rows(group).filter(|row| !self.is_done(row))
    }

    pub fn open_ticked_bytes(&self, group: Group) -> u64 {
        self.open_ticked_rows(group)
            .fold(0, |sum, row| sum.saturating_add(row.bytes))
    }

    pub fn open_ticked_count(&self, group: Group) -> usize {
        self.open_ticked_rows(group).count()
    }

    /// What the last run freed out of the rows of the group.
    pub fn freed_in(&self, group: Group) -> u64 {
        let Some(result) = &self.result else {
            return 0;
        };
        self.group_rows(group)
            .filter_map(|row| match result.logged(&row.path)?.outcome {
                Ok(freed) => Some(freed),
                Err(_) => None,
            })
            .fold(0, u64::saturating_add)
    }

    /// Takes the measure the result held back.
    pub(crate) fn settle_result(&mut self, home: &Path, store_build: bool) {
        self.result = None;
        if let Some(scan) = self.pending_scan.take() {
            self.take_scan(scan, home, store_build);
        }
    }

    /// Replaces the scan, keeping the unticks of rows that are still there.
    pub(crate) fn take_scan(&mut self, scan: Arc<Scan>, home: &Path, store_build: bool) {
        if self.result.is_some() {
            self.pending_scan = Some(scan);
            return;
        }
        self.rows = rows_of(&scan, home, store_build);
        self.scan = Some(scan);
        let rows = &self.rows;
        self.unticked
            .retain(|path| rows.iter().any(|row| &row.path == path));
        if self.cursor_index().is_none() {
            self.cursor = self.items().first().map(|item| self.key_of(*item));
        }
    }

    /// Raises every stop flag: the measure and the run end at their next check.
    pub fn stop_all(&self) {
        if let Some(measuring) = &self.measuring {
            measuring.stop.store(true, Ordering::Relaxed);
        }
        if let Some(running) = &self.running {
            running.stop.store(true, Ordering::Relaxed);
        }
    }
}
