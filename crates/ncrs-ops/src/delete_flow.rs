//! The delete: asking, then one entry per job so Escape works between two.

use std::path::PathBuf;
use std::sync::Arc;

use iced::Task;
use ncrs_core::PanelSide;
use ncrs_fs as fs;
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_jobs::{Job, JobKind};

use crate::action::{Action, StartJob};
use crate::archive_flow::start_archive_delete;
use crate::ctx::FlowCtx;
use crate::delete::default_focus;
use crate::message::OpsMessage;
use crate::state::{Deletion, Ops};

/// What a flow gives back: the work to run and what it asks of the app.
pub type Outcome = (Task<OpsMessage>, Vec<Action>);

pub fn idle() -> Outcome {
    (Task::none(), Vec::new())
}

pub fn actions(actions: Vec<Action>) -> Outcome {
    (Task::none(), actions)
}

/// The outcome with `action` ahead of the others.
pub fn with_front(action: Action, (task, mut rest): Outcome) -> Outcome {
    rest.insert(0, action);
    (task, rest)
}

/// What F8 or Shift+F8 found to delete, read by the app from its panels.
#[derive(Debug)]
pub struct DeleteRequest {
    pub permanent: bool,
    pub sources: Vec<PathBuf>,
    pub side: PanelSide,
    /// The panel holds an archive: it has no trash, its entries go for good.
    pub in_archive: bool,
}

/// F8 / Shift+F8: ask before deleting. Nothing to act on, or something else
/// already holding the keyboard or the queue, means nothing happens.
pub fn open_delete(ops: &mut Ops, request: DeleteRequest, ctx: &FlowCtx<'_>) -> Outcome {
    if ctx.start_blocked || ops.delete_dialog.is_some() || request.sources.is_empty() {
        return idle();
    }
    let permanent = request.permanent || request.in_archive;
    ops.delete_dialog = Some(Deletion {
        sources: request.sources,
        permanent,
        side: request.side,
    });
    actions(vec![
        Action::ClearStatus,
        Action::FocusDialogButton(default_focus(permanent)),
    ])
}

/// The dialog's subject line: the name for one entry, the count for more.
///
/// A permanent delete also lists the first names, because tagged rows may
/// lie outside the visible part of the panel and the user is about to lose
/// them for good.
pub fn delete_subject(lang: Language, deletion: &Deletion) -> String {
    const LISTED: usize = 5;
    let name_of = |path: &PathBuf| {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    match deletion.sources.as_slice() {
        [only] => lang.text(Msg::DeleteOne).replace(
            "{name}",
            &only
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        ),
        many => {
            let mut lines = vec![lang
                .text(Msg::DeleteMany)
                .replace("{count}", &many.len().to_string())];
            if deletion.permanent {
                lines.extend(many.iter().take(LISTED).map(name_of));
                if many.len() > LISTED {
                    lines.push(
                        lang.text(Msg::DeleteMore)
                            .replace("{count}", &(many.len() - LISTED).to_string()),
                    );
                }
            }
            lines.join("\n")
        }
    }
}

/// The dialog was confirmed: start on the first entry.
pub fn confirm_delete(ops: &mut Ops, ctx: &FlowCtx<'_>) -> Outcome {
    let Some(deletion) = ops.delete_dialog.take() else {
        return idle();
    };
    if let Some(archive) = ctx.archive_of(deletion.side) {
        return start_archive_delete(ops, archive.to_path_buf(), deletion, ctx);
    }
    if !deletion.permanent {
        ops.last_deletion.clear();
    }
    ops.deleting = Some(deletion);
    ops.bump_generation();
    run_delete_row(ops, 0, ctx)
}

/// One entry of a delete is done. Continue with the next, or end the delete.
pub fn delete_row_done(
    ops: &mut Ops,
    result: Result<(), String>,
    index: usize,
    generation: u64,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    if generation != ops.generation {
        return idle();
    }
    let (task, mut rest) = match result {
        Ok(()) => {
            remember_trashed(ops, index);
            run_delete_row(ops, index + 1, ctx)
        }
        Err(reason) => {
            let text = delete_failure(ops, ctx, index, &reason);
            let (task, mut ended) = end_delete(ops, ctx);
            ended.insert(0, Action::ShowError(text));
            (task, ended)
        }
    };
    rest.insert(0, Action::FinishJob);
    (task, rest)
}

/// Deletes the entry at `index` on the blocking pool, or finishes when there
/// is none. One entry per job, so Escape works between two entries.
fn run_delete_row(ops: &mut Ops, index: usize, ctx: &FlowCtx<'_>) -> Outcome {
    let Some(deletion) = ops.deleting.clone() else {
        return idle();
    };
    let Some(path) = deletion.sources.get(index).cloned() else {
        let mut ended = end_delete(ops, ctx);
        if let Some(text) = moved_to_trash_notice(ctx, &deletion) {
            ended.1.insert(0, Action::ShowNotice(text));
        }
        return ended;
    };
    let total = deletion.sources.len();
    let generation = ops.generation;
    let trash = Arc::clone(ctx.trash);
    let job = Job {
        kind: JobKind::Delete,
        path: path.clone(),
        total: Some(total),
    };
    let build = Box::new(move || {
        Task::perform(
            tokio::task::spawn_blocking(move || {
                if deletion.permanent {
                    fs::remove_permanently(&path).map_err(|err| err.to_string())
                } else {
                    trash.trash(&path)
                }
            }),
            move |outcome| OpsMessage::DeleteRowDone {
                result: outcome.unwrap_or_else(|join| Err(join.to_string())),
                index,
                generation,
            },
        )
    });
    actions(vec![
        Action::SetJobProgress(fs::transfer::Tick { done: index, total }),
        Action::StartJob(StartJob { job, build }),
    ])
}

/// Ends the delete, finished, failed or stopped: the tags refer to entries
/// that may be gone, so they go, and both panels are re-read.
pub fn end_delete(ops: &mut Ops, ctx: &FlowCtx<'_>) -> Outcome {
    let side = ops.deleting.take().map_or(ctx.source_side, |d| d.side);
    ops.bump_generation();
    actions(vec![
        Action::ClearJobProgress,
        Action::ClearSelection(side),
        Action::ReloadPanels,
        Action::RestartSearch,
    ])
}

/// Notes that the entry at `index` of a delete went to the trash, for the
/// key that puts the latest deletion back. A permanent delete has nothing
/// to put back.
fn remember_trashed(ops: &mut Ops, index: usize) {
    let Some(deletion) = ops.deleting.as_ref().filter(|d| !d.permanent) else {
        return;
    };
    if let Some(path) = deletion.sources.get(index) {
        ops.last_deletion.push(path.clone());
    }
}

/// The notice after a delete that moved everything to a trash that can give it
/// back.
fn moved_to_trash_notice(ctx: &FlowCtx<'_>, deletion: &Deletion) -> Option<String> {
    if deletion.permanent || ctx.trash.reach() == fs::TrashReach::FinderOnly {
        return None;
    }
    let count = deletion.sources.len().to_string();
    Some(match &ctx.restore_last_key {
        Some(key) => fill_template(
            ctx.lang.text(Msg::TrashMovedWithKey),
            &[("{count}", &count), ("{key}", key)],
        ),
        None => fill_template(ctx.lang.text(Msg::TrashMoved), &[("{count}", &count)]),
    })
}

/// The status line for an entry that could not be deleted. A failed trash
/// points at the permanent-delete key rather than falling back to it:
/// deleting for good is the user's decision, not the app's.
pub fn delete_failure(ops: &Ops, ctx: &FlowCtx<'_>, index: usize, reason: &str) -> String {
    let permanent = ops.deleting.as_ref().is_some_and(|d| d.permanent);
    let name = ops
        .deleting
        .as_ref()
        .and_then(|d| d.sources.get(index))
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let permanent_key = &ctx.permanent_delete_key;
    let template = match (permanent, permanent_key) {
        (true, _) => Msg::ErrorDeleteFailed,
        (false, Some(_)) => Msg::ErrorTrashFailed,
        (false, None) => Msg::ErrorTrashFailedNoKey,
    };
    fill_template(
        ctx.lang.text(template),
        &[
            ("{name}", &name),
            ("{reason}", reason),
            ("{key}", permanent_key.as_deref().unwrap_or_default()),
        ],
    )
}
