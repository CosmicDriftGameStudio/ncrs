//! Putting entries back from the trash: one entry per job, so Escape works
//! between two.

use std::sync::Arc;

use iced::Task;
use ncrs_fs as fs;
use ncrs_i18n::{fill_template, Msg};
use ncrs_jobs::{Job, JobKind};

use crate::action::{Action, StartJob};
use crate::conflict::OVERWRITE_FOCUS;
use crate::ctx::FlowCtx;
use crate::delete_flow::{idle, Outcome};
use crate::message::{ConflictChoice, OpsMessage, TrashMessage};
use crate::state::{Ops, Restoration};

fn actions(actions: Vec<Action>) -> Outcome {
    (Task::none(), actions)
}

/// The outcome with `action` ahead of the others.
fn with_front(action: Action, (task, mut rest): Outcome) -> Outcome {
    rest.insert(0, action);
    (task, rest)
}

/// Puts one trashed entry back. A folder the sandbox cannot reach is refused
/// first, before the entry in the way is trashed.
pub fn restore_one(
    trash: &dyn fs::Trash,
    sandbox: &fs::SandboxAccess,
    item: &fs::TrashedItem,
    overwrite: bool,
) -> Result<(), fs::RestoreError> {
    let folder = item.original.parent().unwrap_or(&item.original);
    if !sandbox.reaches(folder) {
        return Err(fs::RestoreError::NotGranted);
    }
    if overwrite {
        trash
            .trash(&item.original)
            .map_err(fs::RestoreError::Failed)?;
    }
    trash.restore(item)
}

/// Of each path the session trashed, the newest entry of the trash from that
/// place. Blocking: comparing places resolves folders on the disk.
pub fn newest_entries_from(
    listed: &[fs::TrashedItem],
    trashed: &[std::path::PathBuf],
) -> Vec<fs::TrashedItem> {
    trashed
        .iter()
        .filter_map(|path| {
            listed
                .iter()
                .find(|item| fs::is_same_place(&item.original, path))
                .cloned()
        })
        .collect()
}

/// Whether a restore may start: nothing else holds the queue or the keys.
fn restore_is_blocked(ops: &Ops, ctx: &FlowCtx<'_>) -> bool {
    ctx.start_blocked
        || ops.delete_dialog.is_some()
        || ctx.transfer_dialog_open
        || ops.restoring.is_some()
}

/// Puts `items` back, one per job, so Escape works between two entries.
pub fn start_restore(ops: &mut Ops, items: Vec<fs::TrashedItem>, ctx: &FlowCtx<'_>) -> Outcome {
    if items.is_empty() || restore_is_blocked(ops, ctx) {
        return idle();
    }
    ops.restoring = Some(Restoration {
        items,
        restored: 0,
        skipped: 0,
        rule: None,
    });
    ops.bump_generation();
    with_front(Action::ClearStatus, run_restore_row(ops, 0, false, ctx))
}

/// Puts the entry at `index` back on the blocking pool, or finishes when
/// there is none. With `overwrite`, what is in the way goes to the trash
/// first: nothing is ever deleted for good here.
fn run_restore_row(ops: &mut Ops, index: usize, overwrite: bool, ctx: &FlowCtx<'_>) -> Outcome {
    let Some(restoring) = ops.restoring.as_ref() else {
        return idle();
    };
    let Some(item) = restoring.items.get(index).cloned() else {
        return end_restore(ops, ctx, false);
    };
    let total = restoring.items.len();
    let generation = ops.generation;
    let trash = Arc::clone(ctx.trash);
    let sandbox = ctx.sandbox.clone();
    let job = Job {
        kind: JobKind::Restore,
        path: item.original.clone(),
        total: Some(total),
    };
    let build = Box::new(move || {
        Task::perform(
            tokio::task::spawn_blocking(move || {
                restore_one(trash.as_ref(), &sandbox, &item, overwrite)
            }),
            move |outcome| {
                OpsMessage::Trash(TrashMessage::RestoreRowDone {
                    result: outcome
                        .unwrap_or_else(|join| Err(fs::RestoreError::Failed(join.to_string()))),
                    index,
                    generation,
                })
            },
        )
    });
    actions(vec![
        Action::SetJobProgress(fs::transfer::Tick { done: index, total }),
        Action::StartJob(StartJob { job, build }),
    ])
}

pub fn restore_row_done(
    ops: &mut Ops,
    result: Result<(), fs::RestoreError>,
    index: usize,
    generation: u64,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    if generation != ops.generation {
        return idle();
    }
    let rule = ops.restoring.as_ref().and_then(|restoring| restoring.rule);
    let outcome = match result {
        Ok(()) => {
            if let Some(restoring) = ops.restoring.as_mut() {
                restoring.restored += 1;
            }
            run_restore_row(ops, index + 1, false, ctx)
        }
        Err(fs::RestoreError::Taken) => match rule {
            Some(fs::transfer::OnConflict::Overwrite) => run_restore_row(ops, index, true, ctx),
            Some(_) => skip_restore_row(ops, index, ctx),
            None => {
                ops.restore_conflict = Some(index);
                actions(vec![
                    Action::FocusDialogButton(OVERWRITE_FOCUS),
                    Action::CloseQuickOpenAndPalette,
                ])
            }
        },
        Err(fs::RestoreError::NotGranted) => {
            let name = restore_row_name(ops, index);
            let text = fill_template(
                ctx.lang.text(Msg::TrashRestoreNotGranted),
                &[("{name}", &name)],
            );
            with_front(
                Action::ShowErrorWithFinderPress(text),
                end_restore(ops, ctx, true),
            )
        }
        Err(fs::RestoreError::Failed(reason)) => {
            let name = restore_row_name(ops, index);
            let params = [("{name}", name.as_str()), ("{reason}", reason.as_str())];
            let shown = if cfg!(target_os = "macos") {
                Action::ShowErrorWithFinderPress(fill_template(
                    ctx.lang.text(Msg::TrashRestoreFailedFinder),
                    &params,
                ))
            } else {
                Action::ShowError(fill_template(
                    ctx.lang.text(Msg::TrashRestoreFailed),
                    &params,
                ))
            };
            with_front(shown, end_restore(ops, ctx, true))
        }
    };
    with_front(Action::FinishJob, outcome)
}

fn restore_row_name(ops: &Ops, index: usize) -> String {
    ops.restoring
        .as_ref()
        .and_then(|restoring| restoring.items.get(index))
        .and_then(|item| item.original.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn skip_restore_row(ops: &mut Ops, index: usize, ctx: &FlowCtx<'_>) -> Outcome {
    if let Some(restoring) = ops.restoring.as_mut() {
        restoring.skipped += 1;
    }
    run_restore_row(ops, index + 1, false, ctx)
}

/// The answer to the conflict dialog of a restore.
pub fn resume_restore(
    ops: &mut Ops,
    index: usize,
    choice: ConflictChoice,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    if choice == ConflictChoice::Cancel {
        return end_restore(ops, ctx, false);
    }
    if choice.applies_to_all() {
        if let Some(restoring) = ops.restoring.as_mut() {
            restoring.rule = Some(choice.conflict());
        }
    }
    match choice.conflict() {
        fs::transfer::OnConflict::Overwrite => run_restore_row(ops, index, true, ctx),
        fs::transfer::OnConflict::Skip | fs::transfer::OnConflict::Fail => {
            skip_restore_row(ops, index, ctx)
        }
    }
}

/// Ends the restore, finished, failed or stopped: reloads both panels and
/// the trash, and says what came back unless an error already speaks.
/// `error_set` says the flow has just asked for an error, which the app has
/// not shown yet.
pub fn end_restore(ops: &mut Ops, ctx: &FlowCtx<'_>, error_set: bool) -> Outcome {
    let outcome = ops.restoring.take();
    ops.restore_conflict = None;
    ops.bump_generation();
    let mut asked = vec![Action::ClearConflictAll, Action::ClearJobProgress];
    if let Some(done) = outcome {
        if !ctx.has_job_error && !error_set && (done.restored > 0 || done.skipped > 0) {
            let mut text = fill_template(
                ctx.lang.text(Msg::TrashRestored),
                &[("{count}", &done.restored.to_string())],
            );
            if done.skipped > 0 {
                text.push_str(&fill_template(
                    ctx.lang.text(Msg::TrashRestoredLeft),
                    &[("{skipped}", &done.skipped.to_string())],
                ));
            }
            asked.push(Action::ShowNotice(text));
        }
    }
    asked.extend([
        Action::ReloadPanels,
        Action::ListTrash,
        Action::RestartSearch,
    ]);
    actions(asked)
}

/// The key behind "Restore the last deletion": reads the trash, then puts
/// back what the latest F8 of this session moved there.
pub fn restore_last_deletion(ops: &mut Ops, ctx: &FlowCtx<'_>) -> Outcome {
    if restore_is_blocked(ops, ctx) {
        return idle();
    }
    if ops.last_deletion.is_empty() {
        return actions(vec![Action::ShowNotice(
            ctx.lang.text(Msg::TrashNothingToRestore).to_owned(),
        )]);
    }
    ops.bump_generation();
    let generation = ops.generation;
    let trash = Arc::clone(ctx.trash);
    let trashed = ops.last_deletion.clone();
    let task = Task::perform(
        tokio::task::spawn_blocking(move || {
            trash
                .list()
                .map(|listed| newest_entries_from(&listed, &trashed))
        }),
        move |outcome| {
            OpsMessage::Trash(TrashMessage::LastListed {
                generation,
                result: outcome.unwrap_or_else(|join| Err(join.to_string())),
            })
        },
    );
    (task, Vec::new())
}

/// Puts back the entries the trash still holds of the latest deletion.
pub fn restore_listed_deletion(
    ops: &mut Ops,
    generation: u64,
    result: Result<Vec<fs::TrashedItem>, String>,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    if generation != ops.generation {
        return idle();
    }
    let found = match result {
        Ok(found) => found,
        Err(reason) => {
            let text = fill_template(ctx.lang.text(Msg::TrashError), &[("{reason}", &reason)]);
            return actions(vec![Action::ShowError(text)]);
        }
    };
    // A job started while the trash was read: the deletion stays for the
    // next press.
    if restore_is_blocked(ops, ctx) {
        return idle();
    }
    ops.last_deletion.clear();
    if found.is_empty() {
        return actions(vec![Action::ShowNotice(
            ctx.lang.text(Msg::TrashNothingToRestore).to_owned(),
        )]);
    }
    start_restore(ops, found, ctx)
}
