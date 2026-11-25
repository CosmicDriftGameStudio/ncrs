//! "Clean up": fetches the chosen repositories, lists what is merged and
//! deletes what the person ticked. Nothing without a tick: a worktree with
//! changes stays, and nothing runs with `--force`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use iced::Task;
use ncrs_i18n::Msg;
use ncrs_git::{
    CleanupBranch, CleanupList, CleanupPlan, CleanupReason, CleanupReport, CleanupTarget,
    UndoRecord, UndoSummary,
};
use ncrs_ui_kit::kit::Check;
use ncrs_ui_kit::row_scroll::{row_into_view, FOCUSED_ROW_ID};

use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::Message;
use crate::rows::name_of;
use crate::state::{Scope, State};
use crate::summary::Reading;
use crate::cleanup_tasks;
use crate::update::{enqueue_read, pump_reads, Outcome};

/// How far a Page key moves the cursor.
const PAGE: isize = 10;

/// The id of the scrollable around the entries of the pane.
pub const CLEANUP_LIST_ID: &str = "repos-cleanup-list";

/// What can be ticked: a worktree (its branch goes with it) or a branch.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Offer {
    Worktree(PathBuf),
    Branch(String),
}

/// One entry of one repository.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pick {
    pub repo: PathBuf,
    pub offer: Offer,
}

/// Why an entry is offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfferReason {
    InDefault,
    Gone,
    WorktreeOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub pick: Pick,
    /// The branch.
    pub name: String,
    /// The folder of a worktree.
    pub folder: Option<PathBuf>,
    pub reason: OfferReason,
    /// The remote branch (`origin/feat`) that goes with it when the box for the remote is ticked.
    pub remote: Option<String>,
}

/// What reading one repository gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scan {
    pub list: CleanupList,
    pub default_branch: Option<String>,
    /// Why the fetch failed; the list is then the state of the last fetch.
    pub fetch_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoState {
    Waiting,
    Running,
    Scanned(Box<Scan>),
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Reading,
    Ready,
    Deleting,
    Done,
}

/// What Undo came to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Undone {
    pub branches: usize,
    pub failures: Vec<(PathBuf, String)>,
}

/// A line the cursor can stand on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    Group(PathBuf),
    Item(Pick),
}

#[derive(Debug, Clone, PartialEq)]
pub enum CleanupMessage {
    Open,
    Close,
    Scanned(u64, PathBuf, Box<Result<Scan, String>>),
    Toggle(Pick),
    ToggleGroup(PathBuf),
    FoldGroup(PathBuf),
    CursorTo(Line),
    ToggleKeep,
    ToggleRemote,
    Confirm,
    Deleted(u64, PathBuf, Box<Result<CleanupReport, String>>),
    Undo,
    Undone(u64, Vec<(PathBuf, Result<(), String>)>),
    /// The left key: shuts the group the cursor is in.
    Shut,
    /// The right key: opens the group the cursor is on.
    Expand,
}

/// The pane while it is open.
#[derive(Debug)]
pub struct Cleanup {
    pub scope: Scope,
    /// The repositories in the order of the pane.
    pub order: Vec<PathBuf>,
    pub repos: BTreeMap<PathBuf, RepoState>,
    /// Repositories that wait for a free place, to be read or to be deleted in.
    pub queue: VecDeque<PathBuf>,
    pub running: usize,
    pub phase: Phase,
    /// Everything offered is ticked at first; what is here was unticked.
    pub unticked: BTreeSet<Pick>,
    pub folded: BTreeSet<PathBuf>,
    /// The Keep section shows its text.
    pub keep_open: bool,
    pub delete_remote: bool,
    pub cursor: usize,
    pub fetched_at: Option<SystemTime>,
    pub reports: BTreeMap<PathBuf, Result<CleanupReport, String>>,
    pub undos: Vec<(PathBuf, UndoRecord)>,
    pub undone: Option<Undone>,
    /// The keys are on the list, not on the pane.
    pub list_focus: bool,
    /// Which opening of the pane this is; results of an earlier one are dropped.
    pub generation: u64,
    /// What Enter confirmed, per repository; the deletes run from exactly this.
    pub plans: BTreeMap<PathBuf, CleanupPlan>,
}

impl Cleanup {
    fn new(
        generation: u64,
        scope: Scope,
        order: Vec<PathBuf>,
        repos: BTreeMap<PathBuf, RepoState>,
    ) -> Self {
        let queue = order
            .iter()
            .filter(|path| repos.get(*path) == Some(&RepoState::Waiting))
            .cloned()
            .collect();
        let phase = if repos.values().any(|repo| *repo == RepoState::Waiting) {
            Phase::Reading
        } else {
            Phase::Ready
        };
        Self {
            scope,
            order,
            repos,
            queue,
            running: 0,
            phase,
            unticked: BTreeSet::new(),
            folded: BTreeSet::new(),
            keep_open: false,
            delete_remote: false,
            cursor: 0,
            fetched_at: None,
            reports: BTreeMap::new(),
            undos: Vec::new(),
            undone: None,
            list_focus: false,
            generation,
            plans: BTreeMap::new(),
        }
    }

    pub fn scan(&self, repo: &Path) -> Option<&Scan> {
        match self.repos.get(repo) {
            Some(RepoState::Scanned(scan)) => Some(scan),
            _ => None,
        }
    }

    /// The entries of a repository: worktrees first, then branches.
    pub fn items(&self, repo: &Path) -> Vec<Item> {
        let Some(scan) = self.scan(repo) else {
            return Vec::new();
        };
        let worktrees = scan.list.worktrees.iter().map(|found| Item {
            pick: Pick {
                repo: repo.to_path_buf(),
                offer: Offer::Worktree(found.path.clone()),
            },
            name: found.branch.name.clone(),
            folder: Some(found.path.clone()),
            reason: OfferReason::WorktreeOnly,
            remote: found.branch.remote_tip.as_ref().and(found.branch.upstream.clone()),
        });
        let branches = scan.list.branches.iter().map(|found| Item {
            pick: Pick {
                repo: repo.to_path_buf(),
                offer: Offer::Branch(found.name.clone()),
            },
            name: found.name.clone(),
            folder: None,
            reason: match found.reason {
                CleanupReason::InDefault => OfferReason::InDefault,
                CleanupReason::UpstreamGone => OfferReason::Gone,
            },
            remote: found.remote_tip.as_ref().and(found.upstream.clone()),
        });
        worktrees.chain(branches).collect()
    }

    pub fn is_ticked(&self, pick: &Pick) -> bool {
        !self.unticked.contains(pick)
    }

    pub fn group_check(&self, repo: &Path) -> Check {
        let items = self.items(repo);
        let ticked = items
            .iter()
            .filter(|item| self.is_ticked(&item.pick))
            .count();
        match ticked {
            0 => Check::Off,
            count if count == items.len() => Check::On,
            _ => Check::Mixed,
        }
    }

    fn toggle(&mut self, pick: Pick) {
        if !self.unticked.remove(&pick) {
            self.unticked.insert(pick);
        }
    }

    fn toggle_group(&mut self, repo: &Path) {
        let items = self.items(repo);
        let all_ticked = items.iter().all(|item| self.is_ticked(&item.pick));
        for item in items {
            if all_ticked {
                self.unticked.insert(item.pick);
            } else {
                self.unticked.remove(&item.pick);
            }
        }
    }

    /// The repositories that offer something, in the order of the pane.
    pub fn offering(&self) -> Vec<&Path> {
        self.order
            .iter()
            .map(PathBuf::as_path)
            .filter(|repo| !self.items(repo).is_empty())
            .collect()
    }

    /// Repositories that were read and have nothing to delete.
    pub fn without_offers(&self) -> Vec<&Path> {
        self.order
            .iter()
            .map(PathBuf::as_path)
            .filter(|repo| self.scan(repo).is_some() && self.items(repo).is_empty())
            .collect()
    }

    pub fn lines(&self) -> Vec<Line> {
        let mut lines = Vec::new();
        for repo in self.offering() {
            lines.push(Line::Group(repo.to_path_buf()));
            if !self.folded.contains(repo) {
                lines.extend(
                    self.items(repo)
                        .into_iter()
                        .map(|item| Line::Item(item.pick)),
                );
            }
        }
        lines
    }

    pub fn cursor_line(&self) -> Option<Line> {
        self.lines().into_iter().nth(self.cursor)
    }

    /// How many branches and worktrees are offered, and in how many repositories.
    pub fn offered(&self) -> (usize, usize, usize) {
        let (mut branches, mut worktrees, mut repos) = (0, 0, 0);
        for repo in self.offering() {
            repos += 1;
            if let Some(scan) = self.scan(repo) {
                branches += scan.list.branches.len();
                worktrees += scan.list.worktrees.len();
            }
        }
        (branches, worktrees, repos)
    }

    /// How many entries are ticked, and in how many repositories.
    pub fn ticked(&self) -> (usize, usize) {
        let mut entries = 0;
        let mut repos = 0;
        for repo in self.offering() {
            let here = self
                .items(repo)
                .iter()
                .filter(|item| self.is_ticked(&item.pick))
                .count();
            entries += here;
            repos += usize::from(here > 0);
        }
        (entries, repos)
    }

    /// What `plan_for` would delete from a remote: ticked branches that are in
    /// the default branch and have a remote branch. The count, and the remote.
    pub fn remote_candidates(&self) -> (usize, Option<String>) {
        let mut count = 0;
        let mut remote = None;
        for repo in self.offering() {
            let Some(plan) = self.plan_for(repo) else {
                continue;
            };
            let branches = plan
                .branches
                .iter()
                .chain(plan.worktrees.iter().map(|found| &found.branch));
            for branch in branches.filter(|branch| branch.remote_tip.is_some()) {
                if let Some(name) = &branch.upstream {
                    count += 1;
                    remote.get_or_insert_with(|| {
                        name.split_once('/')
                            .map_or(name.clone(), |(first, _)| first.to_owned())
                    });
                }
            }
        }
        (count, remote)
    }

    /// What is ticked in `repo`; none when nothing is.
    pub fn plan_for(&self, repo: &Path) -> Option<CleanupPlan> {
        let scan = self.scan(repo)?;
        let ticked = |offer: Offer| {
            self.is_ticked(&Pick {
                repo: repo.to_path_buf(),
                offer,
            })
        };
        let branches: Vec<CleanupBranch> = scan
            .list
            .branches
            .iter()
            .filter(|found| ticked(Offer::Branch(found.name.clone())))
            .cloned()
            .collect();
        let worktrees: Vec<_> = scan
            .list
            .worktrees
            .iter()
            .filter(|found| ticked(Offer::Worktree(found.path.clone())))
            .cloned()
            .collect();
        (!branches.is_empty() || !worktrees.is_empty()).then_some(CleanupPlan {
            branches,
            worktrees,
            delete_remote: self.delete_remote,
        })
    }

    /// Repositories that have an end result for reading or deleting.
    pub fn read_count(&self) -> usize {
        self.repos
            .values()
            .filter(|repo| !matches!(repo, RepoState::Waiting | RepoState::Running))
            .count()
    }

    /// What happened to an entry: `None` while nothing was deleted, else its
    /// result, with the reason git gave when it failed.
    pub fn result_of(&self, item: &Item) -> Option<Result<(), String>> {
        let report = self.reports.get(&item.pick.repo)?;
        let report = match report {
            Ok(report) => report,
            Err(reason) => return Some(Err(reason.clone())),
        };
        let targets: Vec<CleanupTarget> = match &item.pick.offer {
            Offer::Worktree(path) => vec![
                CleanupTarget::Worktree(path.clone()),
                CleanupTarget::Branch(item.name.clone()),
            ],
            Offer::Branch(name) => vec![CleanupTarget::Branch(name.clone())],
        };
        let mut seen = false;
        for row in report.rows.iter().filter(|row| targets.contains(&row.target)) {
            seen = true;
            if let Err(error) = &row.result {
                return Some(Err(crate::bulk::error_line(error)));
            }
        }
        seen.then_some(Ok(()))
    }

    /// How many entries were deleted.
    pub fn deleted_count(&self) -> usize {
        self.offering()
            .into_iter()
            .flat_map(|repo| self.items(repo))
            .filter(|item| matches!(self.result_of(item), Some(Ok(()))))
            .count()
    }

    fn clamp_cursor(&mut self) {
        let last = self.lines().len().saturating_sub(1);
        self.cursor = self.cursor.min(last);
    }

    fn move_cursor(&mut self, delta: isize) {
        let last = self.lines().len().saturating_sub(1);
        self.cursor = self.cursor.saturating_add_signed(delta).min(last);
    }

    fn group_of_cursor(&self) -> Option<PathBuf> {
        match self.cursor_line()? {
            Line::Group(repo) => Some(repo),
            Line::Item(pick) => Some(pick.repo),
        }
    }

    fn put_cursor_on_group(&mut self, repo: &Path) {
        let place = self
            .lines()
            .iter()
            .position(|line| *line == Line::Group(repo.to_path_buf()));
        if let Some(index) = place {
            self.cursor = index;
        }
    }
}

fn quiet(task: Task<Message>) -> Outcome {
    (task, Vec::new())
}

fn scroll() -> Task<Message> {
    row_into_view(CLEANUP_LIST_ID, FOCUSED_ROW_ID)
}

fn limit(ctx: &Ctx) -> usize {
    usize::from(ctx.settings.concurrency).max(1)
}

fn open(state: &mut State, ctx: &Ctx) -> Outcome {
    let busy = state.run.as_ref().is_some_and(|run| !run.is_finished());
    if !ctx.actions_available || busy || state.cleanup.is_some() || state.clone.is_some() {
        return quiet(Task::none());
    }
    let (scope, mut targets) = state.targets();
    targets.sort_by_cached_key(|path| (name_of(path).to_lowercase(), path.clone()));
    let repos: BTreeMap<PathBuf, RepoState> = targets
        .iter()
        .map(|path| {
            let known = match state.readings.get(path) {
                Some(Reading::Unreadable(why)) => RepoState::Failed(why.clone()),
                Some(Reading::Gone) => {
                    RepoState::Failed(ctx.lang.text(Msg::ReposFolderGone).to_owned())
                }
                Some(Reading::Read(_)) | None => RepoState::Waiting,
            };
            (path.clone(), known)
        })
        .collect();
    if repos.is_empty() {
        return quiet(Task::none());
    }
    state.menu = None;
    state.filter_focused = false;
    state.cleanup_generation += 1;
    state.cleanup = Some(Cleanup::new(state.cleanup_generation, scope, targets, repos));
    quiet(pump_scans(state, ctx))
}

fn pump_scans(state: &mut State, ctx: &Ctx) -> Task<Message> {
    let mut started = Vec::new();
    while let Some(cleanup) = state.cleanup.as_mut() {
        if cleanup.phase != Phase::Reading || cleanup.running >= limit(ctx) {
            break;
        }
        let Some(path) = cleanup.queue.pop_front() else {
            break;
        };
        cleanup.running += 1;
        cleanup.repos.insert(path.clone(), RepoState::Running);
        started.push(cleanup_tasks::scan(cleanup.generation, path, Arc::clone(&state.stop)));
    }
    Task::batch(started)
}

fn pump_deletes(state: &mut State, ctx: &Ctx) -> Task<Message> {
    let mut started = Vec::new();
    while let Some(cleanup) = state.cleanup.as_mut() {
        if cleanup.phase != Phase::Deleting || cleanup.running >= limit(ctx) {
            break;
        }
        let Some(path) = cleanup.queue.pop_front() else {
            break;
        };
        let Some(plan) = cleanup.plans.get(&path).cloned() else {
            continue;
        };
        cleanup.running += 1;
        started.push(cleanup_tasks::delete(cleanup.generation, path, plan));
    }
    Task::batch(started)
}

fn scanned(
    state: &mut State,
    ctx: &Ctx,
    generation: u64,
    path: PathBuf,
    result: Result<Scan, String>,
) -> Outcome {
    let Some(cleanup) = state.cleanup.as_mut().filter(|cleanup| {
        cleanup.generation == generation
            && cleanup.phase == Phase::Reading
            && cleanup.repos.get(&path) == Some(&RepoState::Running)
    }) else {
        return quiet(Task::none());
    };
    cleanup.running = cleanup.running.saturating_sub(1);
    cleanup.repos.insert(
        path.clone(),
        match result {
            Ok(scan) => RepoState::Scanned(Box::new(scan)),
            Err(reason) => RepoState::Failed(reason),
        },
    );
    cleanup.fetched_at = Some(ctx.now);
    if cleanup.queue.is_empty() && cleanup.running == 0 {
        cleanup.phase = Phase::Ready;
    }
    cleanup.clamp_cursor();
    enqueue_read(state, path);
    quiet(Task::batch([pump_scans(state, ctx), pump_reads(state, ctx)]))
}

fn confirm(state: &mut State, ctx: &Ctx) -> Outcome {
    let Some(cleanup) = state.cleanup.as_mut() else {
        return quiet(Task::none());
    };
    if cleanup.phase != Phase::Ready || cleanup.ticked().0 == 0 {
        return quiet(Task::none());
    }
    cleanup.phase = Phase::Deleting;
    let plans: BTreeMap<PathBuf, CleanupPlan> = cleanup
        .order
        .iter()
        .filter_map(|path| Some((path.clone(), cleanup.plan_for(path)?)))
        .collect();
    cleanup.queue = cleanup
        .order
        .iter()
        .filter(|path| plans.contains_key(*path))
        .cloned()
        .collect();
    cleanup.plans = plans;
    quiet(pump_deletes(state, ctx))
}

fn deleted(
    state: &mut State,
    ctx: &Ctx,
    generation: u64,
    path: PathBuf,
    result: Result<CleanupReport, String>,
) -> Outcome {
    let Some(cleanup) = state.cleanup.as_mut().filter(|cleanup| {
        cleanup.generation == generation
            && cleanup.phase == Phase::Deleting
            && cleanup.plans.contains_key(&path)
            && !cleanup.reports.contains_key(&path)
    }) else {
        return quiet(Task::none());
    };
    cleanup.running = cleanup.running.saturating_sub(1);
    let mut actions = Vec::new();
    if let Ok(report) = &result {
        for row in report.rows.iter().filter(|row| row.result.is_ok()) {
            if let CleanupTarget::Worktree(folder) = &row.target {
                actions.push(Action::WorktreeRemoved(folder.clone()));
            }
        }
        if report.rows.iter().any(|row| row.result.is_ok()) {
            actions.push(Action::RepoChanged(path.clone()));
        }
        if let Some(undo) = report.undo.clone() {
            cleanup.undos.push((path.clone(), undo));
        }
    }
    cleanup.reports.insert(path.clone(), result);
    if cleanup.queue.is_empty() && cleanup.running == 0 {
        cleanup.phase = Phase::Done;
    }
    enqueue_read(state, path);
    (
        Task::batch([pump_deletes(state, ctx), pump_reads(state, ctx)]),
        actions,
    )
}

fn undo(state: &State) -> Outcome {
    let Some(cleanup) = state.cleanup.as_ref() else {
        return quiet(Task::none());
    };
    if cleanup.phase != Phase::Done || cleanup.undos.is_empty() {
        return quiet(Task::none());
    }
    quiet(cleanup_tasks::undo(cleanup.generation, cleanup.undos.clone()))
}

fn undone(
    state: &mut State,
    ctx: &Ctx,
    generation: u64,
    results: Vec<(PathBuf, Result<(), String>)>,
) -> Outcome {
    let Some(cleanup) = state
        .cleanup
        .as_mut()
        .filter(|cleanup| cleanup.generation == generation && cleanup.phase == Phase::Done)
    else {
        return quiet(Task::none());
    };
    let mut outcome = Undone::default();
    let mut actions = Vec::new();
    let mut touched = Vec::new();
    for (path, result) in results {
        match result {
            Ok(()) => {
                for (_, record) in cleanup.undos.iter().filter(|(root, _)| *root == path) {
                    if let UndoSummary::CleanUp { branches } = &record.summary {
                        outcome.branches += branches;
                    }
                }
                cleanup.undos.retain(|(root, _)| *root != path);
                cleanup.reports.remove(&path);
                actions.push(Action::RepoChanged(path.clone()));
                touched.push(path);
            }
            Err(reason) => outcome.failures.push((path, reason)),
        }
    }
    cleanup.undone = Some(outcome);
    for path in touched {
        enqueue_read(state, path);
    }
    (pump_reads(state, ctx), actions)
}

/// The keys while the pane is open; any other message comes back.
fn key(state: &mut State, message: Message, ctx: &Ctx) -> Result<Outcome, Message> {
    if state.cleanup.is_none() || state.clone.is_some() || state.filter_focused {
        return Err(message);
    }
    match message {
        Message::SwitchFocus => {
            if let Some(cleanup) = state.cleanup.as_mut() {
                cleanup.list_focus = !cleanup.list_focus;
            }
            return Ok(quiet(Task::none()));
        }
        Message::Escape if state.menu.is_none() => {
            return Ok(handle(state, CleanupMessage::Close, ctx))
        }
        _ => {}
    }
    if state.cleanup.as_ref().is_some_and(|cleanup| cleanup.list_focus) {
        return Err(message);
    }
    match message {
        Message::Enter => return Ok(handle(state, CleanupMessage::Confirm, ctx)),
        Message::Left => return Ok(handle(state, CleanupMessage::Shut, ctx)),
        Message::Right => return Ok(handle(state, CleanupMessage::Expand, ctx)),
        _ => {}
    }
    let Some(cleanup) = state.cleanup.as_mut() else {
        return Err(message);
    };
    Ok(match message {
        Message::Up => {
            cleanup.move_cursor(-1);
            quiet(scroll())
        }
        Message::Down => {
            cleanup.move_cursor(1);
            quiet(scroll())
        }
        Message::PageUp => {
            cleanup.move_cursor(-PAGE);
            quiet(scroll())
        }
        Message::PageDown => {
            cleanup.move_cursor(PAGE);
            quiet(scroll())
        }
        Message::First => {
            cleanup.cursor = 0;
            quiet(scroll())
        }
        Message::Last => {
            cleanup.cursor = cleanup.lines().len().saturating_sub(1);
            quiet(scroll())
        }
        Message::Mark => {
            if cleanup.phase == Phase::Ready {
                match cleanup.cursor_line() {
                    Some(Line::Group(repo)) => cleanup.toggle_group(&repo),
                    Some(Line::Item(pick)) => cleanup.toggle(pick),
                    None => {}
                }
            }
            quiet(Task::none())
        }
        Message::Bulk(_) | Message::MarkAll => quiet(Task::none()),
        other => return Err(other),
    })
}

/// Handles the messages of the pane and the keys while it is open; any other
/// message comes back.
pub fn update(state: &mut State, message: Message, ctx: &Ctx) -> Result<Outcome, Message> {
    match message {
        Message::Cleanup(inner) => Ok(handle(state, inner, ctx)),
        other => key(state, other, ctx),
    }
}

fn handle(state: &mut State, message: CleanupMessage, ctx: &Ctx) -> Outcome {
    if let CleanupMessage::Open = message {
        return open(state, ctx);
    }
    let Some(cleanup) = state.cleanup.as_mut() else {
        return quiet(Task::none());
    };
    match message {
        CleanupMessage::Close => {
            if cleanup.phase != Phase::Deleting {
                state.cleanup = None;
            }
            quiet(Task::none())
        }
        CleanupMessage::Scanned(generation, path, result) => {
            scanned(state, ctx, generation, path, *result)
        },
        CleanupMessage::Toggle(pick) => {
            if cleanup.phase == Phase::Ready {
                cleanup.toggle(pick);
            }
            quiet(Task::none())
        }
        CleanupMessage::ToggleGroup(repo) => {
            if cleanup.phase == Phase::Ready {
                cleanup.toggle_group(&repo);
            }
            quiet(Task::none())
        }
        CleanupMessage::FoldGroup(repo) => {
            if !cleanup.folded.remove(&repo) {
                cleanup.folded.insert(repo);
            }
            cleanup.clamp_cursor();
            quiet(Task::none())
        }
        CleanupMessage::CursorTo(line) => {
            if let Some(index) = cleanup.lines().iter().position(|found| *found == line) {
                cleanup.cursor = index;
            }
            quiet(Task::none())
        }
        CleanupMessage::ToggleKeep => {
            cleanup.keep_open = !cleanup.keep_open;
            quiet(Task::none())
        }
        CleanupMessage::ToggleRemote => {
            if cleanup.phase == Phase::Ready {
                cleanup.delete_remote = !cleanup.delete_remote;
            }
            quiet(Task::none())
        }
        CleanupMessage::Confirm => confirm(state, ctx),
        CleanupMessage::Deleted(generation, path, result) => {
            deleted(state, ctx, generation, path, *result)
        },
        CleanupMessage::Undo => undo(state),
        CleanupMessage::Undone(generation, results) => undone(state, ctx, generation, results),
        CleanupMessage::Shut => {
            if let Some(repo) = cleanup.group_of_cursor() {
                cleanup.folded.insert(repo.clone());
                cleanup.put_cursor_on_group(&repo);
            }
            quiet(scroll())
        }
        CleanupMessage::Expand => {
            if let Some(Line::Group(repo)) = cleanup.cursor_line() {
                cleanup.folded.remove(&repo);
            }
            quiet(Task::none())
        }
        CleanupMessage::Open => quiet(Task::none()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ncrs_git::CleanupWorktree;

    fn branch(name: &str, reason: CleanupReason, upstream: Option<&str>) -> CleanupBranch {
        CleanupBranch {
            name: name.to_owned(),
            sha: "abc".to_owned(),
            reason,
            upstream: upstream.map(str::to_owned),
            tip_time: 0,
            remote_tip: upstream.filter(|_| reason == CleanupReason::InDefault).map(|_| "def".to_owned()),
        }
    }

    fn pane() -> (Cleanup, PathBuf) {
        let repo = PathBuf::from("/work/app");
        let list = CleanupList {
            branches: vec![
                branch("done", CleanupReason::InDefault, Some("origin/done")),
                branch("gone", CleanupReason::UpstreamGone, Some("origin/gone")),
            ],
            worktrees: vec![CleanupWorktree {
                path: PathBuf::from("/work/app-wt"),
                branch: branch("wt", CleanupReason::InDefault, None),
            }],
            kept: Vec::new(),
        };
        let scan = Scan {
            list,
            default_branch: Some("main".to_owned()),
            fetch_error: None,
        };
        let repos = BTreeMap::from([(repo.clone(), RepoState::Scanned(Box::new(scan)))]);
        (Cleanup::new(1, Scope::Favourites(1), vec![repo.clone()], repos), repo)
    }

    #[test]
    fn everything_offered_is_ticked_and_a_group_untick_clears_it() {
        let (mut cleanup, repo) = pane();
        assert_eq!(cleanup.group_check(&repo), Check::On);
        assert_eq!(cleanup.ticked(), (3, 1));
        cleanup.toggle_group(&repo);
        assert_eq!(cleanup.group_check(&repo), Check::Off);
        assert!(cleanup.plan_for(&repo).is_none());
    }

    #[test]
    fn only_ticked_entries_go_into_the_plan_and_remote_is_off_by_default() {
        let (mut cleanup, repo) = pane();
        cleanup.toggle(Pick {
            repo: repo.clone(),
            offer: Offer::Branch("gone".to_owned()),
        });
        let plan = cleanup.plan_for(&repo).unwrap_or_default();
        assert_eq!(plan.branches.len(), 1);
        assert_eq!(plan.worktrees.len(), 1);
        assert!(!plan.delete_remote);
        assert_eq!(cleanup.group_check(&repo), Check::Mixed);
    }

    #[test]
    fn remote_candidates_count_only_ticked_branches_in_the_default_branch() {
        let (cleanup, _) = pane();
        assert_eq!(cleanup.remote_candidates(), (1, Some("origin".to_owned())));
    }
}
