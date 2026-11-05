use std::sync::{Arc, PoisonError};
use std::time::Duration;

use iced::widget::operation;
use iced::Task;
use ncrs_ui_kit::focus::release_focus;
use ncrs_ui_kit::layout::ROW_HEIGHT;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::kill::{KillFailure, KillTarget};
use crate::layout::{visible_rows, ROWS_TOP};
use crate::message::MonitorMessage;
use crate::sampler::{Depth, Sampler};
use crate::snapshot::{Snapshot, TaskRow};
use crate::state::{program_folder, Failure, KillDialog, Menu, MenuEntry, SharedSampler, State};
use crate::tasks::{clamp_command_offset, shifted_command_offset};
use crate::view::FILTER_ID;

/// CPU use needs two readings, at least 200 ms apart.
const SECOND_MEASUREMENT_AFTER: Duration = Duration::from_millis(250);

type Outcome = (Task<MonitorMessage>, Option<Action>);

/// How long between two ticks, or `None` for no tick at all.
pub fn tick_every(state: &State) -> Option<Duration> {
    state.depth().map(|_| state.refresh_every())
}

pub fn update(state: &mut State, message: MonitorMessage, ctx: &Ctx) -> Outcome {
    use MonitorMessage as M;
    let visible = visible_rows(ctx.window_size.height);
    match message {
        M::Toggle if state.open => close(state),
        M::Toggle => open(state),
        M::Close => close(state),
        M::Tick | M::MeasureAgain => (measure(state), None),
        M::Measured(depth, snapshot) => {
            let wants_second_reading = lacks_cpu_reading(state, depth, &snapshot);
            take_snapshot(state, depth, *snapshot, visible);
            if wants_second_reading {
                (second_measurement_later(), None)
            } else {
                none()
            }
        }
        M::DrivesMeasured(rows) => {
            if state.open {
                state.drive_rows = Some(rows);
            }
            none()
        }
        M::MeasureFailed(reason) => {
            state.failure = Some(Failure::Measure(reason));
            none()
        }
        M::Move(by) => {
            move_cursor(state, by, visible);
            none()
        }
        M::PageMove(pages) => {
            move_cursor(state, pages.saturating_mul(visible as isize), visible);
            none()
        }
        M::First => {
            state.put_cursor_on_row(0);
            state.window.ensure_visible(0, state.order.len(), visible);
            none()
        }
        M::Last => {
            let last = state.order.len().saturating_sub(1);
            state.put_cursor_on_row(last);
            state
                .window
                .ensure_visible(last, state.order.len(), visible);
            none()
        }
        M::ShiftCommands(direction) => {
            let first = state.window.offset(state.order.len(), visible);
            let longest = state.longest_command(first, first + visible);
            state.command_offset = shifted_command_offset(state.command_offset, direction, longest);
            none()
        }
        M::Scroll(delta) => {
            let mut cursor = state.cursor_index;
            state
                .window
                .scroll(delta, state.order.len(), visible, &mut cursor);
            state.put_cursor_on_row(cursor);
            none()
        }
        M::SelectRow(pid) => {
            state.menu = None;
            if let Some(row) = row_of(state, pid) {
                state.put_cursor_on_row(row);
            }
            (check_filter_focus(), None)
        }
        M::ToggleGroups => {
            state.toggle_grouping();
            keep_command_offset_in_range(state, visible);
            none()
        }
        M::ClickedAway => (check_filter_focus(), None),
        M::FilterFocusChecked(focused) => {
            state.filter_focused = focused;
            none()
        }
        M::SortBy(column) => {
            state.column = column;
            state.restart_list();
            (check_filter_focus(), None)
        }
        M::FilterChanged(filter) => {
            state.filter = filter;
            state.filter_focused = true;
            state.restart_list();
            keep_command_offset_in_range(state, visible);
            none()
        }
        M::FocusFilter => {
            state.filter_focused = true;
            (operation::focus(FILTER_ID), None)
        }
        M::FilterDone => {
            state.filter_focused = false;
            (release_focus(), None)
        }
        M::Escape => escape(state),
        M::AskKill => {
            if let Some(task) = state.task_under_cursor().cloned() {
                ask_kill(state, &task);
            }
            none()
        }
        M::KillFocus => {
            if let Some(dialog) = state.kill_dialog.as_mut() {
                dialog.focus = 1 - dialog.focus;
            }
            none()
        }
        M::DialogEnter => match state.kill_dialog.as_ref().map(|dialog| dialog.focus) {
            Some(1) => confirm_kill(state),
            _ => {
                state.kill_dialog = None;
                none()
            }
        },
        M::ConfirmKill => confirm_kill(state),
        M::CancelKill => {
            state.kill_dialog = None;
            none()
        }
        M::Killed { name, result } => {
            report_kill(state, &name, result);
            (measure(state), None)
        }
        M::OpenMenu(pid) => {
            if let Some(row) = row_of(state, pid) {
                state.put_cursor_on_row(row);
            }
            if let Some(task) = state.task_by_pid(pid).cloned() {
                open_menu(state, task, ctx.pointer, ctx);
            }
            (check_filter_focus(), None)
        }
        M::OpenMenuAtCursor => {
            if let Some(task) = state.task_under_cursor().cloned() {
                let row = state
                    .cursor_index
                    .saturating_sub(state.window.offset(state.order.len(), visible));
                let at = iced::Point::new(
                    ctx.window_size.width / 3.0,
                    (ROWS_TOP + ROW_HEIGHT * (row as f32 + 1.0)).min(ctx.window_size.height),
                );
                open_menu(state, task, at, ctx);
            }
            none()
        }
        M::MenuKey => {
            if state.kill_dialog.is_some() || state.filter_focused {
                none()
            } else if state.menu.is_some() {
                update(state, M::MenuClose, ctx)
            } else {
                update(state, M::OpenMenuAtCursor, ctx)
            }
        }
        M::MenuHover(entry) => {
            if let Some(menu) = state.menu.as_mut() {
                menu.selected = entry;
            }
            none()
        }
        M::MenuMove(by) => {
            let count = state.menu_entries().len() as isize;
            if let Some(menu) = state.menu.as_mut() {
                menu.selected = (menu.selected as isize + by).rem_euclid(count.max(1)) as usize;
            }
            none()
        }
        M::MenuChoose(entry) => choose_entry(state, entry),
        M::MenuActivate => match state.menu.as_ref().map(|menu| menu.selected) {
            Some(selected) => choose_entry(state, selected),
            None => none(),
        },
        M::MenuClose => {
            state.menu = None;
            (check_filter_focus(), None)
        }
        M::OpenDrive(folder) => {
            let (close_task, _) = close(state);
            (close_task, Some(Action::GoToFolder { folder, file: None }))
        }
    }
}

/// A click can take the focus from the filter field without telling anyone, so
/// the flag is read back from the toolkit instead of being kept in step.
fn check_filter_focus() -> Task<MonitorMessage> {
    operation::is_focused(FILTER_ID).map(MonitorMessage::FilterFocusChecked)
}

fn none() -> Outcome {
    (Task::none(), None)
}

fn row_of(state: &State, pid: u32) -> Option<usize> {
    state
        .order
        .iter()
        .position(|&at| state.tasks().get(at).is_some_and(|task| task.pid == pid))
}

fn open(state: &mut State) -> Outcome {
    state.open = true;
    state.column = crate::tasks::TaskColumn::from_setting(state.settings.sort);
    state.filter.clear();
    state.filter_focused = false;
    state.cursor_index = 0;
    state.grouped = false;
    state.groups.clear();
    state.window = ncrs_ui_kit::list_window::ListWindow::default();
    state.command_offset = 0;
    state.menu = None;
    state.kill_dialog = None;
    state.failure = None;
    state.notice = None;
    state.reorder();
    (measure(state), Some(Action::Opened))
}

/// The first full reading has no CPU percentage; the page asks for the next
/// one only once the first has arrived, because a measurement still running
/// would swallow an earlier request.
pub fn lacks_cpu_reading(state: &State, depth: Depth, snapshot: &Snapshot) -> bool {
    state.open
        && depth == Depth::Full
        && snapshot
            .cpu
            .as_ref()
            .is_some_and(|cpu| cpu.percent.is_none())
}

fn second_measurement_later() -> Task<MonitorMessage> {
    Task::perform(
        async { tokio::time::sleep(SECOND_MEASUREMENT_AFTER).await },
        |()| MonitorMessage::MeasureAgain,
    )
}

fn close(state: &mut State) -> Outcome {
    if !state.open {
        return none();
    }
    state.open = false;
    state.menu = None;
    state.kill_dialog = None;
    state.filter_focused = false;
    state.drive_rows = None;
    state.release_if_idle();
    (release_focus(), Some(Action::Closed))
}

/// Esc closes one layer: the dialog, the menu, the filter field, the page.
fn escape(state: &mut State) -> Outcome {
    if state.kill_dialog.take().is_some() || state.menu.take().is_some() {
        none()
    } else if state.filter_focused {
        state.filter_focused = false;
        state.filter.clear();
        state.restart_list();
        (release_focus(), None)
    } else {
        close(state)
    }
}

fn measure(state: &mut State) -> Task<MonitorMessage> {
    let Some(depth) = state.depth() else {
        return Task::none();
    };
    let mut tasks = Vec::new();
    if let Some(guard) = state.measuring.enter() {
        let slot: SharedSampler = Arc::clone(state.sampler.get_or_insert_with(Default::default));
        tasks.push(Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    let _running = guard;
                    measure_with(&slot, depth)
                })
                .await
            },
            move |joined| match joined {
                Ok(Ok(snapshot)) => MonitorMessage::Measured(depth, Box::new(snapshot)),
                Ok(Err(reason)) => MonitorMessage::MeasureFailed(reason),
                Err(err) => MonitorMessage::MeasureFailed(err.to_string()),
            },
        ));
    }
    if depth == Depth::Full {
        if let Some(run) = state.drives.start() {
            tasks.push(Task::perform(
                async move { tokio::task::spawn_blocking(move || run.run()).await },
                |joined| match joined {
                    Ok(rows) => MonitorMessage::DrivesMeasured(rows),
                    Err(err) => MonitorMessage::MeasureFailed(err.to_string()),
                },
            ));
        }
    }
    Task::batch(tasks)
}

fn measure_with(slot: &SharedSampler, depth: Depth) -> Result<Snapshot, String> {
    let mut sampler = slot.lock().unwrap_or_else(PoisonError::into_inner);
    if sampler.is_none() {
        *sampler = Some(Sampler::open()?);
    }
    sampler
        .as_mut()
        .map(|open| open.measure(depth))
        .ok_or_else(|| String::from("no sampler"))
}

fn take_snapshot(state: &mut State, depth: Depth, snapshot: Snapshot, visible: usize) {
    if state.depth() != Some(depth) {
        return;
    }
    state.snapshot = Some(snapshot);
    state.failure = None;
    if state.open {
        state.reorder();
        keep_command_offset_in_range(state, visible);
    }
}

fn keep_command_offset_in_range(state: &mut State, visible: usize) {
    let first = state.window.offset(state.order.len(), visible);
    let longest = state.longest_command(first, first + visible);
    state.command_offset = clamp_command_offset(state.command_offset, longest);
}

fn move_cursor(state: &mut State, by: isize, visible: usize) {
    let target = state.cursor_index.saturating_add_signed(by);
    state.put_cursor_on_row(target);
    state
        .window
        .ensure_visible(state.cursor_index, state.order.len(), visible);
}

fn ask_kill(state: &mut State, task: &TaskRow) {
    if !crate::state::KILL_OFFERED || !state.entry_is_enabled(MenuEntry::Kill, task) {
        return;
    }
    state.kill_dialog = Some(KillDialog {
        target: KillTarget {
            pid: task.pid,
            start_time: task.start_time,
            name: task.name.clone(),
        },
        focus: 0,
    });
}

fn confirm_kill(state: &mut State) -> Outcome {
    let Some(dialog) = state.kill_dialog.take() else {
        return none();
    };
    let Some(slot) = state.sampler.as_ref().map(Arc::clone) else {
        return none();
    };
    let name = dialog.target.name.clone();
    let task = Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                let mut sampler = slot.lock().unwrap_or_else(PoisonError::into_inner);
                sampler
                    .as_mut()
                    .map_or(Err(KillFailure::NotKilled), |open| {
                        open.kill(&dialog.target)
                    })
            })
            .await
        },
        move |joined| MonitorMessage::Killed {
            name: name.clone(),
            result: joined.unwrap_or(Err(KillFailure::NotKilled)),
        },
    );
    (task, None)
}

fn report_kill(state: &mut State, name: &str, result: Result<(), KillFailure>) {
    state.failure = None;
    state.notice = None;
    match result {
        Ok(()) => state.notice = Some(name.to_owned()),
        Err(failure) => {
            state.failure = Some(Failure::Kill {
                name: name.to_owned(),
                failure,
            });
        }
    }
}

fn open_menu(state: &mut State, task: TaskRow, at: iced::Point, ctx: &Ctx) {
    let entries = state.menu_entries().len();
    let height = ncrs_ui_kit::popup_menu::popup_menu_height(entries);
    let origin = iced::Point::new(
        at.x.min(ctx.window_size.width - ncrs_ui_kit::popup_menu::POPUP_MENU_WIDTH)
            .max(0.0),
        at.y.min(ctx.window_size.height - height).max(0.0),
    );
    state.menu = Some(Menu {
        task,
        origin,
        selected: 0,
    });
}

fn choose_entry(state: &mut State, selected: usize) -> Outcome {
    let Some(menu) = state.menu.take() else {
        return none();
    };
    let Some(&entry) = state.menu_entries().get(selected) else {
        return none();
    };
    if !state.entry_is_enabled(entry, &menu.task) {
        return none();
    }
    match entry {
        MenuEntry::Kill => {
            ask_kill(state, &menu.task);
            none()
        }
        MenuEntry::CopyCommandLine => (
            Task::none(),
            Some(Action::CopyToClipboard(menu.task.command)),
        ),
        MenuEntry::GoToFolder => {
            let Some((folder, file)) = program_folder(&menu.task) else {
                return none();
            };
            let (close_task, _) = close(state);
            (
                close_task,
                Some(Action::GoToFolder {
                    folder,
                    file: Some(file),
                }),
            )
        }
    }
}
