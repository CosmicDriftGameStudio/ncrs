//! The clean-up dialog of the Git tab: merged branches and worktrees of this
//! repository, what stays, and the deletion of what is ticked. The scan fetches
//! first (`fetch --prune`, never prompting); the remote tips are read before
//! the fetch so a branch merged on the server differs from one that got new
//! commits after its last push.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use iced::Task;
use ncrs_git::{
    cleanup_list, remote_state, remote_tips, ActionError, CleanupList, CleanupPlan, CleanupReport,
    CleanupTarget, Git, RemoteState,
};

use crate::actions::Dialog;
use crate::handlers;
use crate::job::{Done, Job};
use crate::state::State;
use crate::update::{quiet, Outcome};

/// One thing the person can tick.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Pick {
    Branch(String),
    Worktree(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Fetching,
    Ready,
    Deleting,
    Done,
}

/// The line the keys are on: a row of the list or the checkbox for the remote branches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    Item(Pick),
    Remote,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupDialog {
    pub phase: Phase,
    pub list: CleanupList,
    pub ticked: HashSet<Pick>,
    pub cursor: usize,
    pub delete_remote: bool,
    /// The list of what stays is open.
    pub keep_open: bool,
    pub fetch_error: Option<String>,
    pub report: Option<CleanupReport>,
    /// The default branch, for "in {main}".
    pub main: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanupMessage {
    Open,
    Toggle(Pick),
    ToggleRemote,
    ToggleKeep,
    Up,
    Down,
    Space,
    /// Left shuts the list of what stays, Right opens it.
    Fold(bool),
    /// Tab (`false`) and Shift+Tab (`true`) between the list and the remote checkbox.
    Tab(bool),
    Confirm,
}

impl CleanupDialog {
    fn fetching(main: String) -> Self {
        Self {
            phase: Phase::Fetching,
            list: CleanupList::default(),
            ticked: HashSet::new(),
            cursor: 0,
            delete_remote: false,
            keep_open: false,
            fetch_error: None,
            report: None,
            main,
        }
    }

    /// Worktrees first, then branches: the order the backend deletes in.
    pub fn picks(&self) -> Vec<Pick> {
        let worktrees = self
            .list
            .worktrees
            .iter()
            .map(|found| Pick::Worktree(found.path.clone()));
        let branches = self
            .list
            .branches
            .iter()
            .map(|found| Pick::Branch(found.name.clone()));
        worktrees.chain(branches).collect()
    }

    /// Branches whose remote branch may go with them.
    pub fn remote_count(&self) -> usize {
        self.list
            .branches
            .iter()
            .filter(|found| found.remote_tip.is_some())
            .count()
    }

    pub fn lines(&self) -> Vec<Line> {
        let mut lines: Vec<Line> = self.picks().into_iter().map(Line::Item).collect();
        if self.remote_count() > 0 {
            lines.push(Line::Remote);
        }
        lines
    }

    pub fn line_at_cursor(&self) -> Option<Line> {
        self.lines().into_iter().nth(self.cursor)
    }

    pub fn toggle(&mut self, pick: Pick) {
        if self.phase != Phase::Ready {
            return;
        }
        if !self.ticked.remove(&pick) {
            self.ticked.insert(pick);
        }
    }

    pub fn plan(&self) -> CleanupPlan {
        CleanupPlan {
            branches: self
                .list
                .branches
                .iter()
                .filter(|found| self.ticked.contains(&Pick::Branch(found.name.clone())))
                .cloned()
                .collect(),
            worktrees: self
                .list
                .worktrees
                .iter()
                .filter(|found| self.ticked.contains(&Pick::Worktree(found.path.clone())))
                .cloned()
                .collect(),
            delete_remote: self.delete_remote,
        }
    }

    /// What became of a row after the deletion.
    pub fn result_of(&self, pick: &Pick) -> Option<&Result<(), ActionError>> {
        let report = self.report.as_ref()?;
        report
            .rows
            .iter()
            .find(|row| match (&row.target, pick) {
                (CleanupTarget::Branch(name), Pick::Branch(other)) => name == other,
                (CleanupTarget::Worktree(path), Pick::Worktree(other)) => path == other,
                _ => false,
            })
            .map(|row| &row.result)
    }

    pub fn deleted_count(&self) -> usize {
        self.report.as_ref().map_or(0, |report| {
            report.rows.iter().filter(|row| row.result.is_ok()).count()
        })
    }
}

/// Reads the remote tips, fetches, and sorts. A fetch that fails (or that the
/// repository's own config would turn into a program run) leaves the state of
/// the last fetch, with the reason.
pub fn scan(git: &Git) -> Result<Done, ActionError> {
    let stop = Arc::new(AtomicBool::new(false));
    let tips_before: BTreeMap<String, String> = remote_tips(git.root(), &stop)?;
    let fetch = git.fetch(None);
    let fetched = fetch.is_ok();
    let remote: RemoteState = remote_state(git.root(), fetched, &stop)?;
    let overview = ncrs_git::refs(git.root(), &stop)?;
    Ok(Done::CleanupScan {
        list: cleanup_list(&overview, &tips_before, &remote),
        fetch_error: fetch.err().map(|error| error.to_string()),
    })
}

/// The fetch for the scan is done: the dialog shows the list, everything ticked.
pub fn scanned(state: &mut State, list: CleanupList, fetch_error: Option<String>) {
    let Some(Dialog::Cleanup(dialog)) = state.actions.dialog.as_mut() else {
        return;
    };
    if dialog.phase != Phase::Fetching {
        return;
    }
    dialog.list = list;
    dialog.fetch_error = fetch_error;
    dialog.ticked = dialog.picks().into_iter().collect();
    dialog.cursor = 0;
    dialog.phase = Phase::Ready;
}

/// The deletion is done. Returns the folders of the worktrees that went, for the app.
pub fn cleaned(state: &mut State, report: CleanupReport) -> Vec<PathBuf> {
    let gone = report
        .rows
        .iter()
        .filter(|row| row.result.is_ok())
        .filter_map(|row| match &row.target {
            CleanupTarget::Worktree(path) => Some(path.clone()),
            _ => None,
        })
        .collect();
    if report.undo.is_some() {
        state.actions.undo = report.undo.clone();
    }
    if let Some(Dialog::Cleanup(dialog)) = state.actions.dialog.as_mut() {
        dialog.phase = Phase::Done;
        dialog.report = Some(report);
    }
    gone
}

/// Whether the job belongs to the dialog: its failure closes it.
pub const fn is_cleanup_job(job: &Job) -> bool {
    matches!(job, Job::FetchForCleanup | Job::CleanUp(_))
}

fn open(state: &mut State) -> Outcome {
    let idle = state.actions.busy.is_none()
        && state.actions.unavailable.is_none()
        && !state.actions.dialog_open()
        && !state.actions.background_fetch;
    if !idle {
        return quiet(Task::none());
    }
    let main = state
        .overview
        .as_ref()
        .and_then(|overview| overview.main_branch.clone())
        .unwrap_or_else(|| "main".to_owned());
    state.actions.dialog = Some(Dialog::Cleanup(Box::new(CleanupDialog::fetching(main))));
    handlers::start(state, Job::FetchForCleanup)
}

fn confirm(state: &mut State) -> Outcome {
    let Some(Dialog::Cleanup(dialog)) = state.actions.dialog.as_mut() else {
        return quiet(Task::none());
    };
    match dialog.phase {
        Phase::Fetching | Phase::Deleting => quiet(Task::none()),
        Phase::Done => {
            state.actions.dialog = None;
            quiet(Task::none())
        }
        Phase::Ready => {
            let plan = dialog.plan();
            if plan.branches.is_empty() && plan.worktrees.is_empty() {
                return quiet(Task::none());
            }
            dialog.phase = Phase::Deleting;
            handlers::start(state, Job::CleanUp(plan))
        }
    }
}

fn move_cursor(dialog: &mut CleanupDialog, down: bool) {
    let last = dialog.lines().len().saturating_sub(1);
    dialog.cursor = if down {
        (dialog.cursor + 1).min(last)
    } else {
        dialog.cursor.saturating_sub(1)
    };
}

pub fn update(state: &mut State, message: CleanupMessage) -> Outcome {
    if message == CleanupMessage::Open {
        return open(state);
    }
    if message == CleanupMessage::Confirm {
        return confirm(state);
    }
    let Some(Dialog::Cleanup(dialog)) = state.actions.dialog.as_mut() else {
        return quiet(Task::none());
    };
    match message {
        CleanupMessage::Open | CleanupMessage::Confirm => {}
        CleanupMessage::Toggle(pick) => dialog.toggle(pick),
        CleanupMessage::ToggleRemote => {
            if dialog.phase == Phase::Ready {
                dialog.delete_remote = !dialog.delete_remote;
            }
        }
        CleanupMessage::ToggleKeep => dialog.keep_open = !dialog.keep_open,
        CleanupMessage::Up => move_cursor(dialog, false),
        CleanupMessage::Down => move_cursor(dialog, true),
        CleanupMessage::Space => match dialog.line_at_cursor() {
            Some(Line::Item(pick)) => dialog.toggle(pick),
            Some(Line::Remote) => {
                if dialog.phase == Phase::Ready {
                    dialog.delete_remote = !dialog.delete_remote;
                }
            }
            None => {}
        },
        CleanupMessage::Fold(open) => dialog.keep_open = open,
        CleanupMessage::Tab(_) => {
            let remote_line = dialog.picks().len();
            let on_remote = dialog.remote_count() > 0 && dialog.cursor == remote_line;
            dialog.cursor = if on_remote || dialog.remote_count() == 0 {
                0
            } else {
                remote_line
            };
        }
    }
    quiet(Task::none())
}
