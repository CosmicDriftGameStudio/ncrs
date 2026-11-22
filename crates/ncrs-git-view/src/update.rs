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
use crate::actions::{MenuTarget, OpenMenu};
use crate::ctx::Ctx;
use crate::diff_layout::rows_before;
use crate::handlers;
use crate::load;
use crate::message::Message;
use crate::state::{Area, ColumnDrag, Detail, DiffState, Fold, SidebarItem, State, Subject, PAGE};
use crate::view::{DIFF_ID, FILTER_ID};

/// Lines an arrow key scrolls the diff by.
const SCROLL_LINES: f32 = 3.0;

pub type Outcome = (Task<Message>, Option<Action>);

pub fn quiet(task: Task<Message>) -> Outcome {
    (task, None)
}

/// Shows the tab for the repository at `root` and reads it. The same
/// repository again only reads it anew. A repository that is opened takes the
/// folded groups the app remembers for it.
pub fn open(state: &mut State, root: PathBuf, name: String, ctx: &Ctx) -> Task<Message> {
    if state.root.as_ref() != Some(&root) {
        state.close();
        state.root = Some(root);
        state.name = name;
        state.limit = PAGE;
        state.collapsed = ctx
            .folded
            .iter()
            .filter_map(|key| Fold::from_key(key))
            .collect();
    }
    state.problem = None;
    load::everything(state)
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
                    handlers::refresh_sync(state, ctx);
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
        Message::FolderPressed => {
            state.area = Area::Detail;
            quiet(Task::none())
        }
        Message::FilePressed(index) => {
            state.area = Area::Detail;
            state.file_cursor = index;
            quiet(open_diff(state, index))
        }
        Message::ToggleFold(fold) => (Task::none(), toggle_fold(state, fold)),
        Message::ResizePressed(column) => {
            state.column_drag = Some(ColumnDrag {
                column,
                start: ctx.columns,
                origin: None,
                current: ctx.columns,
            });
            quiet(Task::none())
        }
        Message::ResizeMoved(x) => {
            if let Some(drag) = state.column_drag.as_mut() {
                let origin = *drag.origin.get_or_insert(x);
                drag.current = drag.start.dragged(drag.column, x - origin);
            }
            quiet(Task::none())
        }
        Message::ResizeReleased => {
            let resized = state
                .column_drag
                .take()
                .filter(|drag| drag.current != drag.start);
            (
                Task::none(),
                resized.map(|drag| Action::ColumnsResized(drag.current)),
            )
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
        // On a group the arrows fold it first; otherwise they move
        // between the areas, without wrapping around.
        Message::Left => {
            let folded = fold_at_cursor(state, true);
            if folded.is_none() && state.area != Area::Sidebar {
                state.area = state.area.previous();
            }
            (Task::none(), folded)
        }
        Message::Right => {
            let folded = fold_at_cursor(state, false);
            if folded.is_none() && state.area != Area::Detail {
                state.area = state.area.next();
            }
            (Task::none(), folded)
        }
        Message::Enter => enter(state, ctx),
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
                .map(|(root, changed)| Action::ViewFile(root.join(&changed.path)));
            (Task::none(), action)
        }
        Message::Escape => escape(state, ctx),
        Message::Close => {
            state.close();
            (Task::none(), Some(Action::Close))
        }
        Message::OpenMenu(_) | Message::OpenMenuKey if state.actions.unavailable.is_some() => {
            quiet(Task::none())
        }
        Message::OpenMenu(target) => {
            state.actions.menu = Some(OpenMenu { target });
            quiet(Task::none())
        }
        Message::OpenMenuKey => {
            let target = state
                .sidebar_items()
                .get(state.sidebar_cursor)
                .and_then(|item| menu_target_of(*item));
            state.actions.menu = target.map(|chosen| OpenMenu { target: chosen });
            quiet(Task::none())
        }
        Message::CloseMenu => {
            state.actions.menu = None;
            quiet(Task::none())
        }
        Message::Refresh => {
            state.problem = None;
            quiet(load::everything(state))
        }
        forwarded @ (Message::ActionsLoaded(..)
        | Message::JobDone(_)
        | Message::SelectChange(..)
        | Message::Stage(_)
        | Message::Unstage(_)
        | Message::StageAll
        | Message::UnstageAll
        | Message::StageHunk(_)
        | Message::DiscardHunk(_)
        | Message::UnstageHunk(_)
        | Message::DiscardFile(_)
        | Message::DiscardAll
        | Message::CommitTitleChanged(_)
        | Message::CommitBodyEdited(_)
        | Message::ToggleAmend
        | Message::Commit
        | Message::Fetch
        | Message::Pull
        | Message::Push
        | Message::Stash
        | Message::Undo
        | Message::MenuPressed(_)
        | Message::NewBranchNameChanged(_)
        | Message::ToggleNewBranchCheckout
        | Message::CreateBranch
        | Message::CloseDialog
        | Message::ConfirmDialog
        | Message::RenameNameChanged(_)
        | Message::MergeAbort
        | Message::MergeContinue
        | Message::SelectConflict(_)
        | Message::TakeBlock(..)
        | Message::ConflictPressed(..)
        | Message::ConflictShortcut(_)
        | Message::PullFirst
        | Message::PullWith(_)
        | Message::DeleteBranchAnyway
        | Message::Retry
        | Message::ToggleNoticeDetails
        | Message::DismissNotice) => handlers::act(state, forwarded, ctx),
    }
}

pub fn failed(state: &mut State, ctx: &Ctx, reason: &str) -> Outcome {
    state.problem = Some(fill_template(
        ctx.lang.text(Msg::GitViewReadFailed),
        &[("{reason}", reason)],
    ));
    quiet(Task::none())
}

/// Reads the detail of the row under the cursor unless it is the one shown.
pub fn ensure_detail(state: &mut State) -> Task<Message> {
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

/// Folds `fold` if it is open and unfolds it if it is folded; the action tells
/// the app what to remember.
fn toggle_fold(state: &mut State, fold: Fold) -> Option<Action> {
    if !state.collapsed.remove(&fold) {
        state.collapsed.insert(fold);
    }
    state.sidebar_cursor = state
        .sidebar_cursor
        .min(state.sidebar_items().len().saturating_sub(1));
    Some(Action::FoldsChanged {
        root: state.root.clone()?,
        folded: state.folded_keys(),
    })
}

/// Folds or unfolds the group under the cursor; `None` if there is none or it is
/// as asked already.
fn fold_at_cursor(state: &mut State, collapse: bool) -> Option<Action> {
    if state.area != Area::Sidebar {
        return None;
    }
    let item = state.sidebar_items().get(state.sidebar_cursor).copied()?;
    let fold = state.fold_of(item)?;
    if state.collapsed.contains(&fold) == collapse {
        return None;
    }
    toggle_fold(state, fold)
}

/// What a menu on this row of the left list is about.
pub fn menu_target_of(item: SidebarItem) -> Option<MenuTarget> {
    match item {
        SidebarItem::Local(branch) => Some(MenuTarget::LocalBranch(branch)),
        SidebarItem::Remote(branch) => Some(MenuTarget::RemoteBranch(branch)),
        SidebarItem::Stash(stash) => Some(MenuTarget::Stash(stash)),
        _ => None,
    }
}

fn activate_sidebar(state: &mut State, index: usize) -> Outcome {
    let item = state.sidebar_items().get(index).copied();
    if let Some(fold) = item.and_then(|row| state.fold_of(row)) {
        return (Task::none(), toggle_fold(state, fold));
    }
    state.actions.changes_open =
        state.actions.local.is_some() && item == Some(SidebarItem::Changes);
    match item {
        Some(SidebarItem::Worktree(position)) => {
            let path = state
                .overview
                .as_ref()
                .and_then(|overview| overview.worktrees.get(position))
                .map(|worktree| worktree.path.clone());
            (Task::none(), path.map(Action::OpenWorktreeTab))
        }
        _ => quiet(Task::none()),
    }
}

fn enter(state: &mut State, ctx: &Ctx) -> Outcome {
    if state.actions.dialog_open() {
        // A text field sends Enter as its own submit too; the second one finds nothing to do.
        let answer = if state.actions.dialog.is_some() {
            Message::ConfirmDialog
        } else {
            Message::CreateBranch
        };
        return handlers::act(state, answer, ctx);
    }
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

fn escape(state: &mut State, ctx: &Ctx) -> Outcome {
    if state.actions.dialog_open() {
        return handlers::act(state, Message::CloseDialog, ctx);
    }
    if state.actions.menu.is_some() {
        state.actions.menu = None;
        return quiet(Task::none());
    }
    if state.column_drag.take().is_some() {
        return quiet(Task::none());
    }
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
