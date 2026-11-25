//! The history of one file and who last changed each of its lines: what they
//! remember, how they open over the commit list, and the menu of a file.

#![allow(
    clippy::wildcard_enum_match_arm,
    reason = "this module takes a group of messages out of the big match in `update`; the rest never arrives here"
)]

use iced::widget::operation;
use iced::widget::scrollable::AbsoluteOffset;
use iced::Task;
use ncrs_git::{
    Blame, BlameGroup, ChangeKind, ChangedFile, DiffTarget, FileDiff, FileHistory,
    FileHistoryEntry, LogCommit,
};
use ncrs_ui_kit::focus::release_focus;
use ncrs_ui_kit::page_scroll::scroll_page_by;
use ncrs_ui_kit::theme::size;

use crate::action::Action;
use crate::actions::{MenuTarget, OpenMenu};
use crate::ctx::Ctx;
use crate::load;
use crate::message::Message;
use crate::navigate;
use crate::state::{Area, DetailRow, DiffState, State, Subject, PAGE};
use crate::update::{
    failed, quiet, reload_diff, scroll_to_hunk, step, Outcome, SCROLL_LINES,
};
use crate::view::{BLAME_ID, COMMITS_ID, HISTORY_DIFF_ID, HISTORY_ID};
use crate::wip;

/// Rows above the cursor of a long list while the keys move it.
const ROWS_ABOVE_CURSOR: usize = 8;

/// The commits that changed one file, over the commit list.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryState {
    pub path: String,
    pub log: Option<FileHistory>,
    pub limit: usize,
    pub cursor: usize,
    /// What the commit under the cursor changed in the file.
    pub diff: Option<FileDiff>,
    /// How far down the list is, in pixels.
    pub scroll: f32,
}

/// Who last changed each line of one file, over everything else in the middle.
#[derive(Debug, Clone, PartialEq)]
pub struct BlameState {
    pub path: String,
    /// The commit the file is taken from; `None`: the last one.
    pub at: Option<String>,
    pub blame: Option<Blame>,
    pub cursor: usize,
    /// How far down the list is, in pixels.
    pub scroll: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileAction {
    History,
    Blame,
    OpenEditor,
    Ignore,
}

impl HistoryState {
    pub fn entry(&self) -> Option<&FileHistoryEntry> {
        self.log.as_ref()?.commits.get(self.cursor)
    }
}

impl BlameState {
    /// The commit the line under the cursor comes from.
    pub fn group(&self) -> Option<&BlameGroup> {
        let blame = self.blame.as_ref()?;
        blame.groups.get(blame.lines.get(self.cursor)?.group)
    }
}

/// What the middle of the tab shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Middle {
    Commits,
    LocalDiff,
    Conflict,
    History,
    Diff,
    Blame,
}

pub fn middle(state: &State) -> Middle {
    if state.blame.is_some() {
        return Middle::Blame;
    }
    if state.diff.is_some() {
        return Middle::Diff;
    }
    if state.history.is_some() {
        return Middle::History;
    }
    if state.actions.merge.is_some() {
        return Middle::Conflict;
    }
    let local = state.actions.local.as_ref();
    if state.changes_shown() && local.is_some_and(|local| local.diff_open) {
        return Middle::LocalDiff;
    }
    Middle::Commits
}

/// The lists of the middle share one scroll area in the widget tree: one that
/// takes the place of another has to go where it was left.
pub fn scroll_into_place(state: &State) -> Task<Message> {
    match middle(state) {
        Middle::Commits => scroll_to(COMMITS_ID, state.commits_scroll),
        Middle::History => {
            let offset = state.history.as_ref().map_or(0.0, |history| history.scroll);
            scroll_to(HISTORY_ID, offset)
        }
        Middle::Blame => {
            let offset = state.blame.as_ref().map_or(0.0, |blame| blame.scroll);
            scroll_to(BLAME_ID, offset)
        }
        Middle::Diff => scroll_to_hunk(state),
        Middle::LocalDiff | Middle::Conflict => Task::none(),
    }
}

/// The keys of the list in the middle go to the blame or the history.
pub fn has_keys(state: &State) -> bool {
    if state.blame.is_some() {
        return state.area == Area::Commits;
    }
    state.diff.is_none()
        && state.history.is_some()
        && matches!(state.area, Area::Commits | Area::Detail)
}

/// The commit the keys mean while the commit list is covered: full id and short id.
pub fn commit_of_layer(state: &State) -> Option<(String, String)> {
    if let Some(blame) = &state.blame {
        return blame
            .group()
            .map(|group| (group.sha.clone(), group.short.clone()));
    }
    if state.diff.is_some() {
        return None;
    }
    let commit: &LogCommit = &state.history.as_ref()?.entry()?.commit;
    Some((commit.id.clone(), commit.short.clone()))
}

/// The commit a file of `subject` is taken from; `None`: the working copy or the last commit.
pub fn commit_of(subject: &Subject) -> Option<String> {
    match subject {
        Subject::Working => None,
        Subject::Commit(sha) => Some(sha.clone()),
        Subject::Compare { to, .. } => to.clone(),
    }
}

/// The file the user is at, with the commit it is shown in.
pub fn current_file(state: &State) -> Option<(String, Option<String>)> {
    if let Some(blame) = &state.blame {
        return Some((blame.path.clone(), blame.at.clone()));
    }
    if let Some(shown) = &state.diff {
        let file = shown.files.get(shown.file)?;
        return Some((file.path.clone(), commit_of(&shown.subject)));
    }
    if let Some(history) = &state.history {
        let entry = history.entry()?;
        return Some((
            entry.path_at_commit.clone(),
            Some(entry.commit.id.clone()),
        ));
    }
    if let Some(merge) = &state.actions.merge {
        let conflict = merge.conflicts.get(merge.selected)?;
        return Some((conflict.path.clone(), None));
    }
    if state.changes_shown() {
        let path = wip::selected_path(state.actions.local.as_ref()?)?;
        return Some((path.to_owned(), None));
    }
    if state.area != Area::Detail {
        return None;
    }
    let file = state.files_in_detail().get(detail_file(state)?)?;
    Some((
        file.path.clone(),
        commit_of(&state.detail.as_ref()?.subject),
    ))
}

/// The file of the detail pane under the cursor.
fn detail_file(state: &State) -> Option<usize> {
    (state.file_cursor < state.files_in_detail().len()).then_some(state.file_cursor)
}

pub fn handle(state: &mut State, message: Message, ctx: &Ctx) -> Outcome {
    match message {
        Message::OpenHistory(path) => quiet(open_history(state, path)),
        Message::OpenBlame(path, at) => quiet(open_blame(state, path, at)),
        Message::HistoryKey => match current_file(state) {
            Some((path, _)) => quiet(open_history(state, path)),
            None => quiet(Task::none()),
        },
        Message::BlameKey => match current_file(state) {
            Some((path, at)) if state.blame.is_none() => quiet(open_blame(state, path, at)),
            _ => quiet(Task::none()),
        },
        Message::HistoryLoaded(generation, result) => {
            if !state.loads.history.is_current(generation) {
                return quiet(Task::none());
            }
            match *result {
                Ok(log) => quiet(show_history(state, log)),
                Err(reason) => failed(state, ctx, &reason),
            }
        }
        Message::HistoryDiffLoaded(generation, result) => {
            if !state.loads.history_diff.is_current(generation) {
                return quiet(Task::none());
            }
            match *result {
                Ok(diff) => {
                    if let Some(history) = state.history.as_mut() {
                        history.diff = Some(diff);
                    }
                    quiet(scroll_to(HISTORY_DIFF_ID, 0.0))
                }
                Err(reason) => failed(state, ctx, &reason),
            }
        }
        Message::BlameLoaded(generation, result) => {
            if !state.loads.blame.is_current(generation) {
                return quiet(Task::none());
            }
            match *result {
                Ok(found) => {
                    if let Some(blame) = state.blame.as_mut() {
                        blame.cursor = step(blame.cursor, 0, found.lines.len());
                        blame.blame = Some(found);
                    }
                    quiet(Task::none())
                }
                Err(reason) => failed(state, ctx, &reason),
            }
        }
        Message::HistoryPressed(row) => {
            state.area = Area::Commits;
            quiet(choose_history_row(state, row))
        }
        Message::HistoryMore => quiet(more_history(state)),
        Message::HistoryScrolled(offset) => {
            if let Some(history) = state.history.as_mut() {
                history.scroll = offset;
            }
            quiet(Task::none())
        }
        Message::BlamePressed(line) => {
            state.area = Area::Commits;
            if let Some(blame) = state.blame.as_mut() {
                blame.cursor = line;
            }
            quiet(Task::none())
        }
        Message::BlameScrolled(offset) => {
            if let Some(blame) = state.blame.as_mut() {
                blame.scroll = offset;
            }
            quiet(Task::none())
        }
        Message::FileMenu(action) => menu_pressed(state, action),
        _ => quiet(Task::none()),
    }
}

/// An arrow or Enter while [`has_keys`].
pub fn key(state: &mut State, message: &Message) -> Task<Message> {
    let delta = match message {
        Message::Up => -1,
        Message::Down => 1,
        _ => return enter(state),
    };
    if let Some(blame) = state.blame.as_mut() {
        let lines = blame.blame.as_ref().map_or(0, |found| found.lines.len());
        blame.cursor = step(blame.cursor, delta, lines);
        blame.scroll = offset_around(blame.cursor);
        return scroll_to(BLAME_ID, blame.scroll);
    }
    if state.area == Area::Detail {
        return scroll_page_by(HISTORY_DIFF_ID, delta as f32 * SCROLL_LINES * size::ROW);
    }
    let Some(history) = state.history.as_mut() else {
        return Task::none();
    };
    let rows = history.log.as_ref().map_or(0, |log| log.commits.len());
    let next = step(history.cursor, delta, rows);
    let at_the_end = delta > 0 && next + 1 >= rows;
    history.scroll = offset_around(next);
    let scrolled = scroll_to(HISTORY_ID, history.scroll);
    let chosen = choose_history_row(state, next);
    let more = if at_the_end {
        more_history(state)
    } else {
        Task::none()
    };
    Task::batch([scrolled, chosen, more])
}

/// Enter: the commit of the line in the list, or the diff of the commit.
fn enter(state: &mut State) -> Task<Message> {
    if state.blame.is_some() {
        return match commit_of_layer(state) {
            Some((sha, _)) => navigate::jump_to(state, sha),
            None => Task::none(),
        };
    }
    let Some(history) = &state.history else {
        return Task::none();
    };
    let Some(entry) = history.entry() else {
        return Task::none();
    };
    let is_first = history
        .log
        .as_ref()
        .is_some_and(|log| !log.has_more && history.cursor + 1 == log.commits.len());
    state.diff = Some(DiffState {
        subject: Subject::Commit(entry.commit.id.clone()),
        files: vec![ChangedFile {
            path: entry.path_at_commit.clone(),
            old_path: entry.old_path.clone(),
            kind: match entry.old_path {
                Some(_) => ChangeKind::Renamed,
                None if is_first => ChangeKind::Added,
                None => ChangeKind::Modified,
            },
            added: 0,
            removed: 0,
            binary: false,
            untracked: false,
        }],
        file: 0,
        diff: None,
        unified: false,
        ignore_whitespace: false,
        hunk: 0,
    });
    reload_diff(state)
}

/// Whether there was one.
pub fn close_blame(state: &mut State) -> bool {
    state.loads.blame.cancel();
    state.blame.take().is_some()
}

/// Escape: closes the blame, or the history when no diff is over it.
pub fn escape_step(state: &mut State) -> bool {
    if close_blame(state) {
        return true;
    }
    if state.diff.is_none() && state.history.take().is_some() {
        state.loads.history.cancel();
        state.loads.history_diff.cancel();
        return true;
    }
    false
}

fn open_history(state: &mut State, path: String) -> Task<Message> {
    state.close_layers();
    state.actions.menu = None;
    state.filter_focused = false;
    state.area = Area::Commits;
    state.history = Some(HistoryState {
        path,
        log: None,
        limit: PAGE,
        cursor: 0,
        diff: None,
        scroll: 0.0,
    });
    Task::batch([
        release_focus(),
        scroll_to(HISTORY_ID, 0.0),
        load::file_history(state),
    ])
}

fn open_blame(state: &mut State, path: String, at: Option<String>) -> Task<Message> {
    state.actions.menu = None;
    state.filter_focused = false;
    state.area = Area::Commits;
    state.blame = Some(BlameState {
        path,
        at,
        blame: None,
        cursor: 0,
        scroll: 0.0,
    });
    Task::batch([release_focus(), scroll_to(BLAME_ID, 0.0), load::blame(state)])
}

/// Reads again what is open.
pub fn reload(state: &mut State) -> Task<Message> {
    let history = if state.history.is_some() {
        load::file_history(state)
    } else {
        Task::none()
    };
    let blame = if state.blame.is_some() {
        load::blame(state)
    } else {
        Task::none()
    };
    Task::batch([history, blame])
}

/// Takes the commits in; the cursor stays on its commit.
fn show_history(state: &mut State, log: FileHistory) -> Task<Message> {
    let Some(history) = state.history.as_mut() else {
        return Task::none();
    };
    let before = history.entry().map(|entry| entry.commit.id.clone());
    let row = before
        .as_ref()
        .and_then(|sha| log.commits.iter().position(|entry| entry.commit.id == *sha));
    history.cursor = row.unwrap_or_default();
    history.log = Some(log);
    if row.is_some() && history.diff.is_some() {
        return Task::none();
    }
    read_history_diff(state)
}

fn choose_history_row(state: &mut State, row: usize) -> Task<Message> {
    let Some(history) = state.history.as_mut() else {
        return Task::none();
    };
    if history.cursor == row && history.diff.is_some() {
        return Task::none();
    }
    history.cursor = row;
    read_history_diff(state)
}

fn read_history_diff(state: &mut State) -> Task<Message> {
    let Some(history) = state.history.as_mut() else {
        return Task::none();
    };
    history.diff = None;
    let Some(entry) = history.entry() else {
        return Task::none();
    };
    let target = DiffTarget {
        commit: Some(entry.commit.id.clone()),
        path: entry.path_at_commit.clone(),
        old_path: entry.old_path.clone(),
    };
    load::history_diff(state, target)
}

fn more_history(state: &mut State) -> Task<Message> {
    let Some(history) = state.history.as_mut() else {
        return Task::none();
    };
    if !history.log.as_ref().is_some_and(|log| log.has_more) {
        return Task::none();
    }
    history.limit += PAGE;
    load::file_history(state)
}

fn offset_around(cursor: usize) -> f32 {
    cursor.saturating_sub(ROWS_ABOVE_CURSOR) as f32 * size::ROW
}

fn scroll_to(id: &'static str, offset: f32) -> Task<Message> {
    operation::scroll_to(
        id,
        AbsoluteOffset {
            x: None,
            y: Some(offset),
        },
    )
}

/// The menu key in the pane at the right: the menu of the file there.
pub fn menu_of_current(state: &State) -> Option<MenuTarget> {
    if state.blame.is_some() || (state.diff.is_none() && state.history.is_some()) {
        return None;
    }
    if let Some(merge) = state.actions.merge.as_ref().filter(|_| state.diff.is_none()) {
        let conflict = merge.conflicts.get(merge.selected)?;
        return Some(MenuTarget::Conflict(conflict.path.clone()));
    }
    let (path, at) = current_file(state)?;
    Some(MenuTarget::File { path, at })
}

/// The lists of files a file menu can be opened on, as they are shown now.
fn shown_files(state: &State) -> Vec<&ChangedFile> {
    if let Some(shown) = &state.diff {
        return shown.files.iter().collect();
    }
    if state.changes_shown() {
        return state.actions.local.as_ref().map_or_else(Vec::new, |local| {
            local.unstaged.iter().chain(&local.staged).collect()
        });
    }
    state.files_in_detail().iter().collect()
}

/// The place of the file among the rows of the pane at the right.
pub fn row_of_file(state: &State, path: &str) -> Option<usize> {
    if !state.diff_has_own_list() && !state.changes_shown() {
        let files = state.files_in_detail();
        return state.detail_rows().iter().position(|row| match row {
            DetailRow::File { index, .. } => files.get(*index).is_some_and(|it| it.path == path),
            DetailRow::Folder { .. } => false,
        });
    }
    shown_files(state).iter().position(|file| file.path == path)
}

/// The entries of the menu of a file and whether they work. "Ignore…" is only
/// there for a file git does not know yet; it writes, so it needs git.
pub fn menu_entries(state: &State, path: &str) -> Vec<(FileAction, bool)> {
    let files = shown_files(state);
    let of_path = |wanted: &dyn Fn(&ChangedFile) -> bool| {
        files.iter().any(|file| file.path == path && wanted(file))
    };
    let untracked = of_path(&|file| file.untracked);
    let gone = of_path(&|file| file.kind == ChangeKind::Deleted);
    let mut entries = vec![
        (FileAction::History, !untracked),
        (FileAction::Blame, !untracked),
        (FileAction::OpenEditor, !gone),
    ];
    if untracked {
        entries.push((
            FileAction::Ignore,
            state.actions.unavailable.is_none() && state.changes_shown(),
        ));
    }
    entries
}

/// A freshly opened menu of a file starts on its first entry that works, not
/// on a greyed one.
pub fn start_menu_on_first_enabled(state: &mut State) {
    let Some(MenuTarget::File { path, .. }) = state
        .actions
        .menu
        .as_ref()
        .map(|open| open.target.clone())
    else {
        return;
    };
    let first = menu_entries(state, &path)
        .iter()
        .position(|(_, enabled)| *enabled)
        .unwrap_or(0);
    if let Some(open) = state.actions.menu.as_mut() {
        open.cursor = first;
    }
}

/// The menu of a file is open: its entries and the cursor.
pub fn menu_is_open(state: &State) -> bool {
    state
        .actions
        .menu
        .as_ref()
        .is_some_and(|open| matches!(open.target, MenuTarget::File { .. }))
}

/// An arrow or Enter while the menu of a file is open.
pub fn menu_key(state: &mut State, message: &Message) -> Outcome {
    state.close_menu_if_stale();
    let Some(OpenMenu {
        target: MenuTarget::File { path, .. },
        cursor,
        ..
    }) = state.actions.menu.clone()
    else {
        return quiet(Task::none());
    };
    let entries = menu_entries(state, &path);
    let delta = match message {
        Message::Up => -1,
        Message::Down => 1,
        Message::Enter => {
            return match entries.get(cursor) {
                Some((action, true)) => menu_pressed(state, *action),
                _ => quiet(Task::none()),
            }
        }
        _ => return quiet(Task::none()),
    };
    if let Some(open) = state.actions.menu.as_mut() {
        open.cursor = step(cursor, delta, entries.len());
    }
    quiet(Task::none())
}

fn menu_pressed(state: &mut State, action: FileAction) -> Outcome {
    state.close_menu_if_stale();
    let Some(OpenMenu {
        target: MenuTarget::File { path, at },
        ..
    }) = state.actions.menu.take()
    else {
        return quiet(Task::none());
    };
    match action {
        FileAction::History => quiet(open_history(state, path)),
        FileAction::Blame => quiet(open_blame(state, path, at)),
        FileAction::OpenEditor => (Task::none(), editor_of(state, &path)),
        FileAction::Ignore => wip::ignore_file(state, &path),
    }
}

pub fn editor_of(state: &State, path: &str) -> Option<Action> {
    let root = state.root.as_ref()?;
    Some(Action::OpenInEditor(root.join(path)))
}
