//! The state changes of the comparison. What reaches outside them comes back
//! as actions.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use iced::Task;
use ncrs_i18n::{fill_template, Msg};

use crate::action::Action;
use crate::core::{Direction, SyncPlan};
use crate::ctx::Ctx;
use crate::message::Message;
use crate::scan::{self, Scan, ScanError};
use crate::state::{AfterSync, Compare, Folders, Run, SyncDialog};

pub type Outcome = (Task<Message>, Vec<Action>);

/// How many rows of a sync list are shown.
pub const COPY_ROWS: usize = 9;
pub const DELETE_ROWS: usize = 4;

pub fn update(compare: &mut Compare, message: Message, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = match message {
        Message::Open => {
            if !ctx.blocking_overlay_open {
                compare.start = Some(Folders {
                    left: ctx.left.to_owned(),
                    right: ctx.right.to_owned(),
                });
                actions.push(Action::CloseContextMenu);
            }
            Task::none()
        }
        Message::ToggleSize => toggle(compare, |choices| &mut choices.size),
        Message::ToggleDate => toggle(compare, |choices| &mut choices.date),
        Message::ToggleContent => toggle(compare, |choices| &mut choices.content),
        Message::ToggleSubfolders => toggle(compare, |choices| &mut choices.subfolders),
        Message::Confirm => {
            if compare.sync.is_some() {
                confirm_sync(compare, ctx, &mut actions)
            } else if let Some(folders) = compare.start.take() {
                let only_differences = compare.run.as_ref().is_some_and(|run| run.only_differences);
                actions.push(Action::MarksChanged);
                start_run(compare, folders, only_differences, ctx)
            } else {
                Task::none()
            }
        }
        Message::Cancel => {
            if compare.sync.take().is_none() {
                compare.start = None;
            }
            Task::none()
        }
        Message::Finished { generation, result } => {
            finished(compare, generation, result, ctx, &mut actions);
            Task::none()
        }
        Message::ToggleOnlyDifferences => {
            if let Some(run) = compare.run.as_mut() {
                run.only_differences = !run.only_differences;
                actions.push(Action::MarksChanged);
            }
            Task::none()
        }
        Message::Again => again(compare, ctx, &mut actions),
        Message::Done => {
            end(compare, &mut actions);
            Task::none()
        }
        Message::OpenSync(direction) => {
            open_sync(compare, direction);
            Task::none()
        }
        Message::ToggleDelete => {
            if let Some(dialog) = compare.sync.as_mut() {
                dialog.delete = !dialog.delete;
            }
            Task::none()
        }
        Message::SyncScrolled { deletions, delta } => {
            scroll_sync(compare, deletions, delta);
            Task::none()
        }
        Message::PanelsChanged => {
            let left_moved = compare
                .run
                .as_ref()
                .is_some_and(|run| run.folders.left != ctx.left || run.folders.right != ctx.right);
            if left_moved {
                end(compare, &mut actions);
            }
            Task::none()
        }
        Message::OpsIdle => match compare.after_sync.take() {
            Some(AfterSync::Copying { deletions, side }) if !deletions.is_empty() => {
                compare.after_sync = Some(AfterSync::Deleting);
                actions.push(Action::Delete {
                    side,
                    sources: deletions,
                });
                Task::none()
            }
            Some(AfterSync::Copying { .. } | AfterSync::Deleting) => {
                again(compare, ctx, &mut actions)
            }
            None => Task::none(),
        },
    };
    (task, actions)
}

fn toggle(
    compare: &mut Compare,
    field: impl FnOnce(&mut crate::core::Choices) -> &mut bool,
) -> Task<Message> {
    if compare.start.is_some() {
        let value = field(&mut compare.choices);
        *value = !*value;
    }
    Task::none()
}

fn end(compare: &mut Compare, actions: &mut Vec<Action>) {
    if compare.run.is_some() {
        actions.push(Action::MarksChanged);
    }
    compare.end();
}

/// Reads the two folders again, with the choices of the dialog.
fn start_run(
    compare: &mut Compare,
    folders: Folders,
    only_differences: bool,
    ctx: &Ctx<'_>,
) -> Task<Message> {
    if let Some(old) = compare.run.take() {
        old.stop.store(true, Ordering::Relaxed);
    }
    compare.generation = compare.generation.wrapping_add(1);
    let generation = compare.generation;
    let stop = Arc::new(AtomicBool::new(false));
    let scanning = Scan {
        pool: Arc::clone(ctx.pool),
        choices: compare.choices,
        stop: Arc::clone(&stop),
    };
    let (left, right) = (folders.left.clone(), folders.right.clone());
    compare.run = Some(Run {
        folders,
        generation,
        stop,
        result: None,
        only_differences,
    });
    Task::perform(scan::compare(scanning, left, right), move |result| {
        Message::Finished { generation, result }
    })
}

fn again(compare: &mut Compare, ctx: &Ctx<'_>, actions: &mut Vec<Action>) -> Task<Message> {
    let Some(run) = compare.run.as_ref() else {
        return Task::none();
    };
    let (folders, only_differences) = (run.folders.clone(), run.only_differences);
    compare.sync = None;
    actions.push(Action::MarksChanged);
    start_run(compare, folders, only_differences, ctx)
}

fn finished(
    compare: &mut Compare,
    generation: u64,
    result: Result<crate::core::Compared, ScanError>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) {
    if compare
        .run
        .as_ref()
        .is_none_or(|run| run.generation != generation)
    {
        return;
    }
    match result {
        Ok(compared) => {
            if let Some(run) = compare.run.as_mut() {
                run.result = Some(compared);
            }
        }
        Err(ScanError::Cancelled) => compare.end(),
        Err(ScanError::Failed(reason)) => {
            compare.end();
            actions.push(Action::ShowError(fill_template(
                ctx.lang.text(Msg::CompareFailed),
                &[("{reason}", &reason)],
            )));
        }
    }
    actions.push(Action::MarksChanged);
}

fn open_sync(compare: &mut Compare, direction: Direction) {
    if compare.compared().is_none() || compare.after_sync.is_some() {
        return;
    }
    compare.sync = Some(SyncDialog {
        direction,
        delete: false,
        copy_window: ncrs_ui_kit::list_window::ListWindow::default(),
        delete_window: ncrs_ui_kit::list_window::ListWindow::default(),
    });
}

fn scroll_sync(compare: &mut Compare, deletions: bool, delta: iced::mouse::ScrollDelta) {
    let Some((_, plan)) = compare.plan() else {
        return;
    };
    let Some(dialog) = compare.sync.as_mut() else {
        return;
    };
    let mut unused_cursor = 0;
    if deletions {
        dialog
            .delete_window
            .scroll(delta, plan.deletions.len(), DELETE_ROWS, &mut unused_cursor);
    } else {
        dialog
            .copy_window
            .scroll(delta, plan.copies.len(), COPY_ROWS, &mut unused_cursor);
    }
}

/// The user pressed the button of the sync dialog: the copies go to the copy
/// queue, and the deletes follow once it is done.
fn confirm_sync(compare: &mut Compare, ctx: &Ctx<'_>, actions: &mut Vec<Action>) -> Task<Message> {
    let Some((direction, plan)) = compare.plan() else {
        compare.sync = None;
        return Task::none();
    };
    let Some(target) = compare
        .run
        .as_ref()
        .map(|run| run.folders.of(direction.target()).to_owned())
    else {
        compare.sync = None;
        return Task::none();
    };
    compare.sync = None;
    let SyncPlan {
        copies,
        deletions: deletion_rows,
    } = plan;
    let deletions: Vec<_> = deletion_rows.into_iter().map(|row| row.path).collect();
    let side = direction.target();
    if copies.is_empty() {
        if deletions.is_empty() {
            return again(compare, ctx, actions);
        }
        compare.after_sync = Some(AfterSync::Deleting);
        actions.push(Action::Delete {
            side,
            sources: deletions,
        });
        return Task::none();
    }
    compare.after_sync = Some(AfterSync::Copying { deletions, side });
    actions.push(Action::Copy {
        sources: copies.into_iter().map(|row| row.path).collect(),
        target,
    });
    Task::none()
}
