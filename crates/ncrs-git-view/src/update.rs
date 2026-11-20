//! `update` of the Git tab: reads, cursors and the diff.

use std::path::PathBuf;

use iced::widget::operation;
use iced::widget::scrollable::AbsoluteOffset;
use iced::Task;
use ncrs_git::{ChangedFile, DiffTarget};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::focus::release_focus;
use ncrs_ui_kit::page_scroll::scroll_page_by;
use ncrs_ui_kit::theme::size;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::diff_layout::rows_before;
use crate::load;
use crate::message::Message;
use crate::state::{Area, Detail, DiffState, Section, SidebarItem, State, Subject, PAGE};
use crate::view::{DIFF_ID, FILTER_ID};

/// Lines an arrow key scrolls the diff by.
const SCROLL_LINES: f32 = 3.0;

type Outcome = (Task<Message>, Option<Action>);

fn quiet(task: Task<Message>) -> Outcome {
    (task, None)
}

/// Shows the tab for the repository at `root` and reads it. The same
/// repository again only reads it anew.
pub fn open(state: &mut State, root: PathBuf, name: String) -> Task<Message> {
    if state.root.as_ref() != Some(&root) {
        state.close();
        state.root = Some(root);
        state.name = name;
        state.limit = PAGE;
    }
    state.problem = None;
    Task::batch([load::refs(state), load::log(state)])
}

pub fn update(state: &mut State, message: Message, ctx: &Ctx) -> Outcome {
    match message {
        Message::RefsLoaded(generation, result) => {
            if !state.loads.refs.is_current(generation) {
                return quiet(Task::none());
            }
            match *result {
                Ok(overview) => {
                    state.overview = Some(overview);
                    state.sidebar_cursor = state
                        .sidebar_cursor
                        .min(state.sidebar_items().len().saturating_sub(1));
                    quiet(ensure_detail(state))
                }
                Err(reason) => failed(state, ctx, &reason),
            }
        }
        Message::LogLoaded(generation, result) => {
            if !state.loads.log.is_current(generation) {
                return quiet(Task::none());
            }
            match *result {
                Ok(log) => {
                    state.log = Some(log);
                    quiet(ensure_detail(state))
                }
                Err(reason) => failed(state, ctx, &reason),
            }
        }
        Message::DetailLoaded(generation, result) => {
            if !state.loads.detail.is_current(generation) {
                return quiet(Task::none());
            }
            match *result {
                Ok(info) => {
                    state.file_cursor = 0;
                    state.detail = Some(Detail {
                        subject: Subject::Commit(info.id.clone()),
                        files: info.files.clone(),
                        info: Some(info),
                    });
                    quiet(Task::none())
                }
                Err(reason) => failed(state, ctx, &reason),
            }
        }
        Message::WorkingLoaded(generation, result) => {
            if !state.loads.detail.is_current(generation) {
                return quiet(Task::none());
            }
            match result {
                Ok(files) => {
                    state.file_cursor = 0;
                    state.detail = Some(Detail {
                        subject: Subject::Working,
                        info: None,
                        files,
                    });
                    quiet(Task::none())
                }
                Err(reason) => failed(state, ctx, &reason),
            }
        }
        Message::DiffLoaded(generation, result) => {
            if !state.loads.diff.is_current(generation) {
                return quiet(Task::none());
            }
            match *result {
                Ok(diff) => {
                    if let Some(shown) = state.diff.as_mut() {
                        shown.diff = Some(diff);
                        shown.hunk = 0;
                    }
                    quiet(operation::scroll_to(
                        DIFF_ID,
                        AbsoluteOffset {
                            x: None,
                            y: Some(0.0),
                        },
                    ))
                }
                Err(reason) => failed(state, ctx, &reason),
            }
        }
        Message::FilterChanged(filter) => {
            state.filter = filter;
            state.limit = PAGE;
            state.commit_cursor = 0;
            state.detail = None;
            quiet(load::log(state))
        }
        Message::FocusFilter => {
            state.filter_focused = true;
            quiet(operation::focus(FILTER_ID))
        }
        Message::FilterSubmit => {
            state.filter_focused = false;
            state.area = Area::Commits;
            quiet(release_focus())
        }
        Message::SidebarPressed(index) => {
            state.area = Area::Sidebar;
            state.sidebar_cursor = index;
            activate_sidebar(state, index)
        }
        Message::CommitPressed(row) => {
            state.area = Area::Commits;
            state.commit_cursor = row;
            quiet(ensure_detail(state))
        }
        Message::FilePressed(index) => {
            state.area = Area::Detail;
            state.file_cursor = index;
            quiet(open_diff(state, index))
        }
        Message::ToggleSection(section) => {
            toggle_section(state, section);
            quiet(Task::none())
        }
        Message::LoadMore => quiet(load_more(state)),
        Message::SetTreeMode(tree) => {
            state.tree_mode = tree;
            quiet(Task::none())
        }
        Message::SetUnified(unified) => {
            if let Some(shown) = state.diff.as_mut() {
                shown.unified = unified;
            }
            quiet(Task::none())
        }
        Message::ToggleIgnoreWhitespace => {
            let Some(shown) = state.diff.as_mut() else {
                return quiet(Task::none());
            };
            shown.ignore_whitespace = !shown.ignore_whitespace;
            quiet(reload_diff(state))
        }
        Message::Up => quiet(move_cursor(state, -1)),
        Message::Down => quiet(move_cursor(state, 1)),
        Message::Left => {
            collapse_at_cursor(state, true);
            quiet(Task::none())
        }
        Message::Right => {
            collapse_at_cursor(state, false);
            quiet(Task::none())
        }
        Message::Enter => enter(state),
        Message::NextArea => {
            state.area = state.area.next();
            quiet(Task::none())
        }
        Message::PreviousArea => {
            state.area = state.area.previous();
            quiet(Task::none())
        }
        Message::NextHunk => quiet(step_hunk(state, true)),
        Message::PreviousHunk => quiet(step_hunk(state, false)),
        Message::NextFile => quiet(step_file(state, 1)),
        Message::PreviousFile => quiet(step_file(state, -1)),
        Message::ViewFile => {
            let file = state
                .diff
                .as_ref()
                .and_then(|shown| shown.files.get(shown.file));
            let action = state
                .root
                .as_ref()
                .zip(file)
                .map(|(root, file)| Action::ViewFile(root.join(&file.path)));
            (Task::none(), action)
        }
        Message::Escape => escape(state),
        Message::Close => {
            state.close();
            (Task::none(), Some(Action::Close))
        }
        Message::Refresh => {
            state.problem = None;
            quiet(Task::batch([load::refs(state), load::log(state)]))
        }
    }
}

fn failed(state: &mut State, ctx: &Ctx, reason: &str) -> Outcome {
    state.problem = Some(fill_template(
        ctx.lang.text(Msg::GitViewReadFailed),
        &[("{reason}", reason)],
    ));
    quiet(Task::none())
}

/// Reads the detail of the row under the cursor unless it is the one shown.
fn ensure_detail(state: &mut State) -> Task<Message> {
    let row = state.commit_cursor.min(state.row_count().saturating_sub(1));
    state.commit_cursor = row;
    let Some(subject) = state.subject_at(row) else {
        return Task::none();
    };
    if state
        .detail
        .as_ref()
        .is_some_and(|detail| detail.subject == subject)
    {
        return Task::none();
    }
    state.detail = None;
    match subject {
        Subject::Working => load::working_changes(state),
        Subject::Commit(sha) => load::commit_detail(state, sha),
    }
}

fn load_more(state: &mut State) -> Task<Message> {
    if !state.log.as_ref().is_some_and(|log| log.has_more) {
        return Task::none();
    }
    state.limit += PAGE;
    load::log(state)
}

fn toggle_section(state: &mut State, section: Section) {
    if !state.collapsed.remove(&section) {
        state.collapsed.insert(section);
    }
    state.sidebar_cursor = state
        .sidebar_cursor
        .min(state.sidebar_items().len().saturating_sub(1));
}

fn collapse_at_cursor(state: &mut State, collapse: bool) {
    if state.area != Area::Sidebar {
        return;
    }
    let item = state.sidebar_items().get(state.sidebar_cursor).copied();
    if let Some(SidebarItem::Header(section)) = item {
        if state.collapsed.contains(&section) == collapse {
            return;
        }
        toggle_section(state, section);
    }
}

fn activate_sidebar(state: &mut State, index: usize) -> Outcome {
    let item = state.sidebar_items().get(index).copied();
    match item {
        Some(SidebarItem::Header(section)) => {
            toggle_section(state, section);
            quiet(Task::none())
        }
        Some(SidebarItem::Worktree(position)) => {
            let path = state
                .overview
                .as_ref()
                .and_then(|overview| overview.worktrees.get(position))
                .map(|worktree| worktree.path.clone());
            (Task::none(), path.map(Action::ShowWorktree))
        }
        _ => quiet(Task::none()),
    }
}

fn enter(state: &mut State) -> Outcome {
    if state.diff.is_some() {
        return quiet(Task::none());
    }
    match state.area {
        Area::Sidebar => {
            let index = state.sidebar_cursor;
            activate_sidebar(state, index)
        }
        Area::Commits => {
            state.area = Area::Detail;
            quiet(Task::none())
        }
        Area::Detail => {
            let index = state.file_cursor;
            quiet(open_diff(state, index))
        }
    }
}

fn step(cursor: usize, delta: isize, len: usize) -> usize {
    cursor
        .saturating_add_signed(delta)
        .min(len.saturating_sub(1))
}

fn move_cursor(state: &mut State, delta: isize) -> Task<Message> {
    if state.diff.is_some() {
        return scroll_page_by(DIFF_ID, delta as f32 * SCROLL_LINES * size::ROW);
    }
    match state.area {
        Area::Sidebar => {
            let len = state.sidebar_items().len();
            state.sidebar_cursor = step(state.sidebar_cursor, delta, len);
            Task::none()
        }
        Area::Commits => {
            let rows = state.row_count();
            state.commit_cursor = step(state.commit_cursor, delta, rows);
            let detail = ensure_detail(state);
            let near_end = state.commit_cursor + 1 >= rows;
            if near_end && delta > 0 {
                Task::batch([detail, load_more(state)])
            } else {
                detail
            }
        }
        Area::Detail => {
            let len = state.files_in_detail().len();
            state.file_cursor = step(state.file_cursor, delta, len);
            Task::none()
        }
    }
}

fn target_of(subject: &Subject, file: &ChangedFile) -> DiffTarget {
    DiffTarget {
        commit: match subject {
            Subject::Commit(sha) => Some(sha.clone()),
            Subject::Working => None,
        },
        path: file.path.clone(),
        old_path: file.old_path.clone(),
    }
}

fn open_diff(state: &mut State, index: usize) -> Task<Message> {
    let Some(detail) = &state.detail else {
        return Task::none();
    };
    if index >= detail.files.len() {
        return Task::none();
    }
    state.diff = Some(DiffState {
        subject: detail.subject.clone(),
        files: detail.files.clone(),
        file: index,
        diff: None,
        unified: false,
        ignore_whitespace: false,
        hunk: 0,
    });
    reload_diff(state)
}

fn reload_diff(state: &mut State) -> Task<Message> {
    let Some(shown) = &state.diff else {
        return Task::none();
    };
    let Some(file) = shown.files.get(shown.file) else {
        return Task::none();
    };
    let target = target_of(&shown.subject, file);
    let ignore_whitespace = shown.ignore_whitespace;
    load::file_diff(state, target, ignore_whitespace)
}

fn step_file(state: &mut State, delta: isize) -> Task<Message> {
    let Some(shown) = state.diff.as_mut() else {
        return Task::none();
    };
    let next = step(shown.file, delta, shown.files.len());
    if next == shown.file {
        return Task::none();
    }
    shown.file = next;
    shown.diff = None;
    state.file_cursor = next;
    reload_diff(state)
}

fn step_hunk(state: &mut State, forward: bool) -> Task<Message> {
    let Some(shown) = state.diff.as_mut() else {
        return Task::none();
    };
    let Some(diff) = &shown.diff else {
        return Task::none();
    };
    let last = diff.hunks.len().saturating_sub(1);
    shown.hunk = if forward {
        (shown.hunk + 1).min(last)
    } else {
        shown.hunk.saturating_sub(1)
    };
    let rows = rows_before(diff, shown.unified, shown.hunk);
    operation::scroll_to(
        DIFF_ID,
        AbsoluteOffset {
            x: None,
            y: Some(rows as f32 * size::ROW),
        },
    )
}

fn escape(state: &mut State) -> Outcome {
    if state.filter_focused {
        state.filter_focused = false;
        return quiet(release_focus());
    }
    if state.diff.take().is_some() {
        state.loads.diff.cancel();
        return quiet(Task::none());
    }
    state.close();
    (Task::none(), Some(Action::Close))
}
