//! The two pickers in the head of the tab: the repository and the branch.
//! What they list and what a choice does; the list itself is drawn by
//! [`crate::view`].

use std::path::PathBuf;

use iced::Task;
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::focus::release_focus;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::handlers;
use crate::job::Job;
use crate::message::Message;
use crate::state::State;
use crate::update::{quiet, Outcome};

/// The id of the search field of an open picker.
pub const PICKER_ID: &str = "git-picker-search";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerKind {
    Repository,
    Branch,
}

/// A repository the repository picker offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoEntry {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picker {
    pub kind: PickerKind,
    pub query: String,
    /// The highlighted row among the rows that [`visible`] lists.
    pub cursor: usize,
    /// The repositories to list: this one, its worktrees, the ones the app knows.
    pub repositories: Vec<RepoEntry>,
}

/// What a row stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    Repository(PathBuf),
    Local(String),
    /// The full name, `origin/main`.
    Remote(String),
    NewBranch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The place among all rows, filtered or not: what a press sends.
    pub id: usize,
    pub choice: Choice,
    pub label: String,
    /// Dim at the right: how far behind its upstream a branch is.
    pub trail: Option<String>,
    /// The repository or branch the tab shows now.
    pub current: bool,
}

fn all_entries(state: &State, picker: &Picker) -> Vec<Entry> {
    let mut found: Vec<(Choice, String, Option<String>, bool)> = Vec::new();
    match picker.kind {
        PickerKind::Repository => {
            for entry in &picker.repositories {
                let current = state.root.as_ref() == Some(&entry.path);
                found.push((
                    Choice::Repository(entry.path.clone()),
                    entry.name.clone(),
                    None,
                    current,
                ));
            }
        }
        PickerKind::Branch => {
            if let Some(overview) = &state.overview {
                for branch in &overview.local {
                    let behind = (branch.behind > 0).then(|| format!("\u{2193}{}", branch.behind));
                    found.push((
                        Choice::Local(branch.name.clone()),
                        branch.name.clone(),
                        behind,
                        branch.is_current,
                    ));
                }
                for full in overview
                    .remote
                    .iter()
                    .filter(|name| !name.ends_with("/HEAD"))
                {
                    found.push((Choice::Remote(full.clone()), full.clone(), None, false));
                }
            }
            found.push((Choice::NewBranch, String::new(), None, false));
        }
    }
    found
        .into_iter()
        .enumerate()
        .map(|(id, (choice, label, trail, current))| Entry {
            id,
            choice,
            label,
            trail,
            current,
        })
        .collect()
}

/// The rows the search leaves; the row for a new branch always stays.
pub fn visible(state: &State) -> Vec<Entry> {
    let Some(picker) = &state.picker else {
        return Vec::new();
    };
    let needle = picker.query.trim().to_lowercase();
    all_entries(state, picker)
        .into_iter()
        .filter(|entry| {
            entry.choice == Choice::NewBranch
                || needle.is_empty()
                || entry.label.to_lowercase().contains(&needle)
        })
        .collect()
}

pub fn open(state: &mut State, kind: PickerKind, ctx: &Ctx) -> Task<Message> {
    let mut repositories: Vec<RepoEntry> = Vec::new();
    if kind == PickerKind::Repository {
        let known = state
            .root
            .iter()
            .map(|path| RepoEntry {
                name: state.name.clone(),
                path: path.clone(),
            })
            .chain(
                state
                    .overview
                    .iter()
                    .flat_map(|overview| overview.worktrees.iter())
                    .map(|worktree| RepoEntry {
                        name: worktree
                            .path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        path: worktree.path.clone(),
                    }),
            )
            .chain(ctx.repositories.iter().cloned());
        for entry in known {
            if !repositories.iter().any(|seen| seen.path == entry.path) {
                repositories.push(entry);
            }
        }
    }
    state.picker = Some(Picker {
        kind,
        query: String::new(),
        cursor: 0,
        repositories,
    });
    let at = visible(state).iter().position(|entry| entry.current);
    if let Some(picker) = state.picker.as_mut() {
        picker.cursor = at.unwrap_or_default();
    }
    state.filter_focused = false;
    iced::widget::operation::focus(PICKER_ID)
}

pub fn close(state: &mut State) -> Task<Message> {
    state.picker = None;
    release_focus()
}

pub fn set_query(state: &mut State, query: String) {
    if let Some(picker) = state.picker.as_mut() {
        picker.query = query;
        picker.cursor = 0;
    }
}

pub fn move_cursor(state: &mut State, delta: isize) {
    let rows = visible(state).len();
    if let Some(picker) = state.picker.as_mut() {
        picker.cursor = picker
            .cursor
            .saturating_add_signed(delta)
            .min(rows.saturating_sub(1));
    }
}

/// Enter on the highlighted row.
pub fn enter(state: &mut State, with_stash: bool, ctx: &Ctx) -> Outcome {
    let rows = visible(state);
    let cursor = state.picker.as_ref().map_or(0, |picker| picker.cursor);
    match rows.get(cursor) {
        Some(entry) => choose(state, entry.clone(), with_stash, ctx),
        None => quiet(Task::none()),
    }
}

/// A press on the row with `id`.
pub fn press(state: &mut State, id: usize, with_stash: bool, ctx: &Ctx) -> Outcome {
    let found = visible(state).into_iter().find(|entry| entry.id == id);
    match found {
        Some(entry) => choose(state, entry, with_stash, ctx),
        None => quiet(Task::none()),
    }
}

fn choose(state: &mut State, entry: Entry, with_stash: bool, ctx: &Ctx) -> Outcome {
    match entry.choice {
        Choice::Repository(path) => {
            let same = state.root.as_ref() == Some(&path);
            let task = close(state);
            (task, (!same).then_some(Action::OpenRepository(path)))
        }
        Choice::NewBranch => {
            let task = close(state);
            let from = handlers::current_branch(state);
            let (asked, action) = handlers::begin_new_branch(state, from);
            (Task::batch([task, asked]), action)
        }
        Choice::Local(_) | Choice::Remote(_) if entry.current => quiet(close(state)),
        Choice::Local(name) => switch(state, name, false, with_stash, ctx),
        Choice::Remote(name) => switch(state, name, true, with_stash, ctx),
    }
}

/// Switches to `branch`. Where the actions are off the list stays open and
/// the hint under the head says why nothing happens.
fn switch(state: &mut State, branch: String, remote: bool, with_stash: bool, ctx: &Ctx) -> Outcome {
    if state.actions.unavailable.is_some() {
        return quiet(Task::none());
    }
    let job = if with_stash {
        Job::StashAndCheckout {
            message: fill_template(
                ctx.lang.text(Msg::GitViewStashMessage),
                &[("{branch}", &handlers::current_branch(state))],
            ),
            branch,
            remote,
        }
    } else if remote {
        Job::CheckoutRemote(branch)
    } else {
        Job::Checkout(branch)
    };
    let closed = close(state);
    let (started, action) = handlers::start(state, job);
    (Task::batch([closed, started]), action)
}
