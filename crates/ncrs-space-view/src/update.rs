use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use iced::widget::operation;
use iced::Task;
use ncrs_fs::transfer::Tick;
use ncrs_space::{Entry, EntryKind, Group, WorktreeSpace};
use ncrs_ui_kit::confirm::ConfirmFocus;
use ncrs_ui_kit::row_scroll::{row_into_view, FOCUSED_ROW_ID};

use crate::action::Action;
use crate::confirm::{ConfirmKind, ConfirmState};
use crate::ctx::Ctx;
use crate::decide::{self, DecideMessage};
use crate::menu::{menu_items, next_enabled};
use crate::message::{MenuEntry, Message};
use crate::rows::{DisplayRow, RowKind};
use crate::state::{CursorKey, Measuring, MenuState, ResultState, Running, State};
use crate::tasks;

/// The id of the scrollable around the groups.
pub const LIST_ID: &str = "space-list";
/// The id of the filter field.
pub const FILTER_ID: &str = "space-filter";

const PAGE_ROWS: usize = 10;

pub type Outcome = (Task<Message>, Vec<Action>);

fn quiet(task: Task<Message>) -> Outcome {
    (task, Vec::new())
}

fn nothing() -> Outcome {
    quiet(Task::none())
}

fn scroll_to_cursor() -> Task<Message> {
    row_into_view(LIST_ID, FOCUSED_ROW_ID)
}

/// Opens the tab and starts measuring the drive.
pub fn open(ctx: &Ctx) -> (State, Task<Message>) {
    let mut state = State::default();
    let task = start_measure(&mut state, ctx);
    (state, task)
}

fn start_measure(state: &mut State, ctx: &Ctx) -> Task<Message> {
    if let Some(measuring) = &state.measuring {
        measuring.stop.store(true, Ordering::Relaxed);
    }
    state.scan_run += 1;
    let stop = Arc::new(AtomicBool::new(false));
    state.measuring = Some(Measuring {
        run: state.scan_run,
        stop: Arc::clone(&stop),
        bytes: 0,
    });
    state.failure = None;
    tasks::measure(state.scan_run, tasks::request_of(ctx), stop)
}

pub fn update(state: &mut State, message: Message, ctx: &Ctx) -> Outcome {
    if state.decide.is_some() {
        if let Some(handled) = update_decide_keys(state, &message, ctx) {
            return handled;
        }
    }
    match message {
        Message::Up => move_cursor(state, -1),
        Message::Down => move_cursor(state, 1),
        Message::PageUp => move_cursor(state, -(PAGE_ROWS as isize)),
        Message::PageDown => move_cursor(state, PAGE_ROWS as isize),
        Message::First => jump(state, false),
        Message::Last => jump(state, true),
        Message::Left => {
            left(state);
            quiet(scroll_to_cursor())
        }
        Message::Right => {
            if let Some(CursorKey::Head(group)) = state.cursor.clone() {
                state.folded.remove(&group);
            }
            nothing()
        }
        Message::Toggle => {
            toggle_at_cursor(state);
            nothing()
        }
        Message::Enter => enter(state, ctx),
        Message::Escape => escape(state),
        Message::FocusFilter => {
            state.filter_focused = true;
            quiet(operation::focus(FILTER_ID))
        }
        Message::FilterChanged(text) => {
            state.filter = text;
            keep_cursor_in_view(state);
            nothing()
        }
        Message::FilterSubmitted => {
            state.filter_focused = false;
            nothing()
        }
        Message::OpenMenuKey => {
            if let Some(row) = state.cursor_row() {
                state.menu = Some(MenuState {
                    path: row.path.clone(),
                    cursor: 0,
                });
            }
            nothing()
        }
        Message::DeleteSafe => open_confirm(state, ConfirmKind::Safe, ctx),
        Message::RemoveWorktrees => open_confirm(state, ConfirmKind::Worktrees, ctx),
        Message::DeleteOnlyThis => open_confirm(state, ConfirmKind::One, ctx),
        Message::Rescan => {
            if state.running.is_some() {
                return nothing();
            }
            quiet(start_measure(state, ctx))
        }
        Message::Retry => retry(state, ctx),
        Message::AskHermes => (Task::none(), vec![Action::AskHermes]),
        Message::GrantHome => (Task::none(), vec![Action::GrantHome]),
        Message::ShowInPanelAtCursor => show_in_panel(state),
        Message::RowPressed(path) => {
            state.cursor = Some(CursorKey::Row(path));
            nothing()
        }
        Message::RowToggled(path) => {
            state.cursor = Some(CursorKey::Row(path.clone()));
            toggle_row(state, &path);
            nothing()
        }
        Message::HeadToggled(group) => {
            state.cursor = Some(CursorKey::Head(group));
            toggle_fold(state, group);
            nothing()
        }
        Message::OpenMenuAt(path) => {
            state.cursor = Some(CursorKey::Row(path.clone()));
            state.menu = Some(MenuState { path, cursor: 0 });
            nothing()
        }
        Message::CloseMenu => {
            state.menu = None;
            nothing()
        }
        Message::MenuCursor(index) => {
            if let Some(menu) = &mut state.menu {
                menu.cursor = index;
            }
            nothing()
        }
        Message::MenuUp => step_menu(state, -1),
        Message::MenuDown => step_menu(state, 1),
        Message::MenuEnter => {
            let picked = state.menu.as_ref().and_then(|menu| {
                menu_items(state)
                    .into_iter()
                    .nth(menu.cursor)
                    .filter(|item| item.enabled)
                    .map(|item| item.entry)
            });
            match picked {
                Some(entry) => update(state, Message::MenuPicked(entry), ctx),
                None => nothing(),
            }
        }
        Message::MenuPicked(entry) => {
            state.menu = None;
            match entry {
                MenuEntry::ShowInPanel => show_in_panel(state),
                MenuEntry::DeleteOnlyThis => open_confirm(state, ConfirmKind::One, ctx),
                MenuEntry::AskHermes => (Task::none(), vec![Action::AskHermes]),
            }
        }
        Message::ListScrolled(offset) => {
            state.list_scroll = offset;
            nothing()
        }
        Message::ScanProgress { run, bytes } => {
            if let Some(measuring) = state.measuring.as_mut().filter(|m| m.run == run) {
                measuring.bytes = bytes;
            }
            nothing()
        }
        Message::Scanned { run, scan } => scanned(state, ctx, run, scan),
        Message::ConfirmCancel => {
            state.confirm = None;
            nothing()
        }
        Message::ConfirmAccept => accept(state, ctx),
        Message::ConfirmEnter => match state.confirm.as_ref().map(|confirm| confirm.focus) {
            Some(ConfirmFocus::Confirm) => accept(state, ctx),
            Some(ConfirmFocus::Cancel) => {
                state.confirm = None;
                nothing()
            }
            None => nothing(),
        },
        Message::ConfirmSwitchFocus => {
            if let Some(confirm) = &mut state.confirm {
                confirm.focus = match confirm.focus {
                    ConfirmFocus::Cancel => ConfirmFocus::Confirm,
                    ConfirmFocus::Confirm => ConfirmFocus::Cancel,
                };
            }
            nothing()
        }
        Message::ConfirmWithWorktreesToggled => {
            if !ctx.store_build {
                if let Some(confirm) = state
                    .confirm
                    .as_mut()
                    .filter(|c| c.kind == ConfirmKind::Safe)
                {
                    confirm.with_worktrees = !confirm.with_worktrees;
                    confirm.refresh_label(ctx.lang);
                }
            }
            nothing()
        }
        Message::RunProgress { run, done } => {
            let Some(running) = state.running.as_mut().filter(|r| r.run == run) else {
                return nothing();
            };
            running.done = done;
            let tick = Tick {
                done,
                total: running.total,
            };
            (Task::none(), vec![Action::SetJobProgress(tick)])
        }
        Message::RunFinished(finished) => run_finished(state, ctx, finished),
        Message::CloseResult => {
            state.result = None;
            nothing()
        }
        Message::ShowLog => {
            let Some(result) = state.result.as_mut() else {
                return nothing();
            };
            result.show_log = true;
            if result.log_lines.is_some() {
                return nothing();
            }
            quiet(tasks::read_log(ctx.log_dir.clone()))
        }
        Message::HideLog => {
            if let Some(result) = state.result.as_mut() {
                result.show_log = false;
            }
            nothing()
        }
        Message::LogRead(lines) => {
            if let Some(result) = state.result.as_mut() {
                result.log_lines = Some(lines);
            }
            nothing()
        }
        Message::Decide(inner) => update_decide(state, inner, ctx),
    }
}

/// While the decide pane is open the keys of the list drive it.
fn update_decide_keys(state: &mut State, message: &Message, ctx: &Ctx) -> Option<Outcome> {
    let inner = match message {
        Message::Up => DecideMessage::Up,
        Message::Down => DecideMessage::Down,
        Message::PageUp => DecideMessage::PageUp,
        Message::PageDown => DecideMessage::PageDown,
        Message::First => DecideMessage::First,
        Message::Last => DecideMessage::Last,
        Message::Toggle => DecideMessage::Toggle,
        Message::Enter => DecideMessage::Enter,
        Message::Left | Message::Right => return Some(nothing()),
        Message::Escape => {
            state.decide = None;
            return Some(nothing());
        }
        Message::DeleteSafe | Message::RemoveWorktrees | Message::DeleteOnlyThis => {
            return Some(nothing());
        }
        _ => return None,
    };
    Some(update_decide(state, inner, ctx))
}

fn update_decide(state: &mut State, message: DecideMessage, ctx: &Ctx) -> Outcome {
    match state.decide.as_mut() {
        Some(pane) => decide::update(pane, message, ctx),
        None => nothing(),
    }
}

fn move_cursor(state: &mut State, step: isize) -> Outcome {
    let count = state.items().len();
    if count == 0 {
        return nothing();
    }
    let at = state.cursor_index().unwrap_or(0);
    state.set_cursor_index(at.saturating_add_signed(step));
    quiet(scroll_to_cursor())
}

fn jump(state: &mut State, to_end: bool) -> Outcome {
    let count = state.items().len();
    state.set_cursor_index(if to_end { count.saturating_sub(1) } else { 0 });
    quiet(scroll_to_cursor())
}

fn keep_cursor_in_view(state: &mut State) {
    if state.cursor_index().is_none() {
        state.set_cursor_index(0);
    }
}

/// On a head: closes the group. On a row: goes to the head of its group.
fn left(state: &mut State) {
    match state.cursor.clone() {
        Some(CursorKey::Head(group)) => {
            state.folded.insert(group);
        }
        Some(CursorKey::Row(path)) => {
            if let Some(group) = state.row_by_path(&path).map(|row| row.group) {
                state.cursor = Some(CursorKey::Head(group));
            }
        }
        None => {}
    }
}

fn toggle_fold(state: &mut State, group: Group) {
    if !state.folded.remove(&group) {
        state.folded.insert(group);
    }
}

fn toggle_row(state: &mut State, path: &std::path::Path) {
    let deletable = state
        .row_by_path(path)
        .is_some_and(DisplayRow::is_deletable);
    if deletable && !state.unticked.remove(path) {
        state.unticked.insert(path.to_path_buf());
    }
}

/// On a row: ticks or unticks it. On a head: ticks the whole group, or unticks it when all of it was ticked.
fn toggle_at_cursor(state: &mut State) {
    match state.cursor.clone() {
        Some(CursorKey::Row(path)) => toggle_row(state, &path),
        Some(CursorKey::Head(group)) => {
            let paths: Vec<PathBuf> = state
                .group_rows(group)
                .filter(|row| row.is_deletable())
                .map(|row| row.path.clone())
                .collect();
            let all_ticked = paths.iter().all(|path| !state.unticked.contains(path));
            for path in paths {
                if all_ticked {
                    state.unticked.insert(path);
                } else {
                    state.unticked.remove(&path);
                }
            }
        }
        None => {}
    }
}

fn enter(state: &mut State, ctx: &Ctx) -> Outcome {
    match state.cursor.clone() {
        Some(CursorKey::Head(group)) => {
            toggle_fold(state, group);
            nothing()
        }
        Some(CursorKey::Row(path)) => {
            let Some(row) = state.row_by_path(&path) else {
                return nothing();
            };
            if row.group != Group::Decide {
                return nothing();
            }
            let is_downloads = row.kind == RowKind::Downloads;
            let (pane, task) = decide::open(path, is_downloads, ctx);
            state.decide = Some(pane);
            quiet(task)
        }
        None => nothing(),
    }
}

/// The first of: the menu, the dialog, the filter, the decide pane, the log, the result, a
/// run or measure that goes on, a filter text. Nothing left: the tab may close.
fn escape(state: &mut State) -> Outcome {
    if state.menu.take().is_some() || state.confirm.take().is_some() {
        return nothing();
    }
    if state.filter_focused {
        state.filter_focused = false;
        return nothing();
    }
    if state.decide.take().is_some() {
        return nothing();
    }
    if let Some(result) = state.result.as_mut() {
        if result.show_log {
            result.show_log = false;
        } else {
            state.result = None;
        }
        return nothing();
    }
    if state.running.is_some() || state.measuring.is_some() {
        state.stop_all();
        return nothing();
    }
    if !state.filter.is_empty() {
        state.filter.clear();
        keep_cursor_in_view(state);
        return nothing();
    }
    (Task::none(), vec![Action::Closed])
}

fn step_menu(state: &mut State, step: isize) -> Outcome {
    let items = menu_items(state);
    if let Some(menu) = &mut state.menu {
        menu.cursor = next_enabled(&items, menu.cursor, step);
    }
    nothing()
}

fn show_in_panel(state: &State) -> Outcome {
    match state.menu_path().or_else(|| state.cursor_path()) {
        Some(path) => (Task::none(), vec![Action::ShowInPanel(path.to_path_buf())]),
        None => nothing(),
    }
}

fn scanned(state: &mut State, ctx: &Ctx, run: u64, scan: Option<Arc<ncrs_space::Scan>>) -> Outcome {
    if state
        .measuring
        .as_ref()
        .is_none_or(|measuring| measuring.run != run)
    {
        return nothing();
    }
    state.measuring = None;
    match scan {
        Some(scan) => {
            state.take_scan(Arc::clone(&scan), &ctx.home, ctx.store_build);
            (Task::none(), vec![Action::Measured(scan)])
        }
        None => nothing(),
    }
}

fn entry_of_row<'a>(state: &'a State, row: &'a DisplayRow) -> impl Iterator<Item = &'a Entry> {
    let entries = state
        .scan
        .as_deref()
        .map(|scan| scan.entries.as_slice())
        .unwrap_or_default();
    row.entries
        .iter()
        .filter_map(move |place| entries.get(*place))
}

fn worktree_of(entry: &Entry) -> Option<WorktreeSpace> {
    match &entry.kind {
        EntryKind::Worktree(worktree) => Some(worktree.clone()),
        EntryKind::Catalog(_) | EntryKind::Decide(_) => None,
    }
}

/// Builds the dialog for what the button takes: the ticked rows, or the row under the cursor.
fn open_confirm(state: &mut State, kind: ConfirmKind, ctx: &Ctx) -> Outcome {
    if state.running.is_some() || state.measuring.is_some() || state.decide.is_some() {
        return nothing();
    }
    if kind == ConfirmKind::Worktrees && ctx.store_build {
        return nothing();
    }
    let rows: Vec<&DisplayRow> = match kind {
        ConfirmKind::Safe => state.ticked_rows(Group::Safe).collect(),
        ConfirmKind::Worktrees => state.ticked_rows(Group::Worktrees).collect(),
        ConfirmKind::One => state
            .cursor_row()
            .filter(|row| row.is_deletable())
            .into_iter()
            .collect(),
    };
    let mut safe = Vec::new();
    let mut worktrees = Vec::new();
    for row in &rows {
        for entry in entry_of_row(state, row) {
            match worktree_of(entry) {
                Some(worktree) => worktrees.push(worktree),
                None => safe.push(entry.clone()),
            }
        }
    }
    if kind == ConfirmKind::Safe {
        worktrees = state
            .ticked_rows(Group::Worktrees)
            .flat_map(|row| entry_of_row(state, row))
            .filter_map(worktree_of)
            .collect();
    }
    if safe.is_empty() && worktrees.is_empty() {
        return nothing();
    }
    state.confirm = Some(ConfirmState::new(
        kind,
        safe,
        worktrees,
        !ctx.processes_known,
        ctx.lang,
    ));
    nothing()
}

/// The skipped rows of the last run, asked once more.
fn retry(state: &mut State, ctx: &Ctx) -> Outcome {
    let Some(result) = &state.result else {
        return nothing();
    };
    let skipped: Vec<PathBuf> = result
        .log
        .rows
        .iter()
        .filter(|row| row.outcome.is_err())
        .map(|row| row.path.clone())
        .collect();
    if skipped.is_empty() || state.running.is_some() || state.measuring.is_some() {
        return nothing();
    }
    let mut safe = Vec::new();
    let mut worktrees = Vec::new();
    for row in state
        .rows
        .iter()
        .filter(|row| row.is_deletable() && skipped.contains(&row.path))
    {
        for entry in entry_of_row(state, row) {
            match worktree_of(entry) {
                Some(worktree) => worktrees.push(worktree),
                None => safe.push(entry.clone()),
            }
        }
    }
    if safe.is_empty() && worktrees.is_empty() {
        return nothing();
    }
    state.confirm = Some(ConfirmState::new(
        ConfirmKind::One,
        safe,
        worktrees,
        !ctx.processes_known,
        ctx.lang,
    ));
    nothing()
}

fn accept(state: &mut State, ctx: &Ctx) -> Outcome {
    let Some(confirm) = state.confirm.take() else {
        return nothing();
    };
    let plan = confirm.plan();
    let total = plan.safe.len() + plan.worktrees.len();
    if total == 0 {
        return nothing();
    }
    state.run_id += 1;
    let stop = Arc::new(AtomicBool::new(false));
    let free_before = state.scan.as_ref().map_or(0, |scan| scan.volume.available);
    state.running = Some(Running {
        run: state.run_id,
        stop: Arc::clone(&stop),
        plan: plan.clone(),
        done: 0,
        total,
        free_before,
    });
    state.result = None;
    let task = tasks::delete(
        state.run_id,
        plan,
        ctx.home.clone(),
        ctx.log_dir.clone(),
        stop,
    );
    let tick = Tick { done: 0, total };
    (task, vec![Action::SetJobProgress(tick)])
}

fn run_finished(state: &mut State, ctx: &Ctx, finished: crate::message::Finished) -> Outcome {
    let Some(running) = state.running.take_if(|running| running.run == finished.run) else {
        return nothing();
    };
    state.failure = finished.log_error;
    state.result = Some(ResultState {
        log: Arc::clone(&finished.log),
        unknown: running.plan.unknown,
        free_before: running.free_before,
        free_after: finished.free_after,
        log_lines: None,
        show_log: false,
    });
    let again = start_measure(state, ctx);
    (
        again,
        vec![Action::ClearJobProgress, Action::RunDone(finished.log)],
    )
}
