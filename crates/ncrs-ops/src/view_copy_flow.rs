//! The copy F3 makes of a file that is not on the disk, an entry of an
//! archive or a file on a server, for the viewer.

use std::path::{Path, PathBuf};

use iced::Task;
use ncrs_fs as fs;
use ncrs_fs::remote::RemotePath;
use ncrs_i18n::{fill_template, Msg};
use ncrs_jobs::{Job, JobKind};
use tokio::sync::mpsc::UnboundedSender;

use crate::action::{Action, StartProgressJob, ViewOrigin};
use crate::ctx::FlowCtx;
use crate::delete_flow::{actions, idle, Outcome};
use crate::message::OpsMessage;
use crate::row_failure::RowFailure;
use crate::state::{Extracting, Ops};
use crate::transfer_flow::refusal_text;

/// Whether a group of view copies is a server's, told by its key: the root
/// address of the server.
fn group_is_server(group: &Path) -> bool {
    RemotePath::parse(group).is_some()
}

/// The job behind F3 on an entry that is not on the disk: makes a slot for
/// the copy under `group`, runs `work` on the blocking pool to fill it, and
/// reports as `ViewExtracted`.
pub fn start_view_copy(
    ops: &mut Ops,
    group: PathBuf,
    shown: PathBuf,
    kind: JobKind,
    work: impl FnOnce(&Path, &UnboundedSender<fs::transfer::Tick>) -> std::io::Result<PathBuf>
        + Send
        + 'static,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    // A transfer or delete under way, or a dialog about to start one, owns
    // the queue and `generation`; unpacking beside it would lose its result.
    if ctx.jobs_busy
        || ops.transfer.is_some()
        || ops.deleting.is_some()
        || ops.delete_dialog.is_some()
        || ctx.transfer_dialog_open
        || ops.conflict_is_open()
    {
        let text = ctx.lang.text(Msg::StatusArchiveViewBusy).to_owned();
        return actions(vec![Action::ShowNotice(text)]);
    }
    let made = ops
        .view_copies
        .as_mut()
        .and_then(|copies| copies.slot(&group).ok());
    let Some(slot) = made else {
        let text = ctx
            .lang
            .text(if group_is_server(&group) {
                Msg::ErrorRemoteViewUnavailable
            } else {
                Msg::ErrorArchiveViewUnavailable
            })
            .to_owned();
        return actions(vec![Action::ShowError(text)]);
    };
    let generation = ops.generation;
    ops.extracting = Some(Extracting {
        archive: group,
        slot: slot.clone(),
    });
    let job = Job {
        kind,
        path: shown,
        total: None,
    };
    let build = Box::new(move |progress_tx| {
        Task::perform(
            tokio::task::spawn_blocking({
                let unpack_into = slot.clone();
                move || work(&unpack_into, &progress_tx)
            }),
            move |outcome| {
                let result = match outcome {
                    Ok(Ok(copy)) => Ok(copy),
                    Ok(Err(err)) => Err(RowFailure::from_io(err)),
                    Err(join) => Err(RowFailure::Other(join.to_string())),
                };
                OpsMessage::ViewExtracted {
                    generation,
                    slot,
                    result,
                }
            },
        )
    });
    actions(vec![
        Action::ClearStatus,
        Action::ResetJobDone,
        Action::ClearJobProgress,
        Action::StartJobWithProgress(StartProgressJob { job, build }),
    ])
}

/// The copy for the viewer is there, or unpacking it failed.
pub fn view_extracted(
    ops: &mut Ops,
    generation: u64,
    slot: PathBuf,
    result: Result<PathBuf, RowFailure>,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    if generation != ops.generation {
        // The unpacking thread removed the slot itself when it failed.
        if result.is_ok() {
            discard_view_slot(ops, &slot);
        }
        return idle();
    }
    let mut asked = vec![Action::FinishJob, Action::ClearJobProgress];
    let finished = ops.extracting.take();
    match result {
        Ok(copy) => {
            let still_open = finished
                .as_ref()
                .is_some_and(|job| ctx.panel_shows(&job.archive));
            if let (true, Some(job)) = (still_open, finished) {
                let origin = if group_is_server(&job.archive) {
                    ViewOrigin::Server
                } else {
                    ViewOrigin::Archive
                };
                asked.push(Action::LaunchViewCopy { path: copy, origin });
            } else {
                discard_view_slot(ops, &slot);
            }
        }
        Err(failure) => {
            if let Some(copies) = ops.view_copies.as_mut() {
                copies.forget_slot(&slot);
            }
            let text = match failure {
                RowFailure::Refused(refusal) => refusal_text(ctx.lang, refusal),
                RowFailure::UnsafeName(name) => fill_template(
                    ctx.lang.text(Msg::ErrorRemoteUnsafeName),
                    &[("{name}", &name)],
                ),
                RowFailure::Conflict(reason) | RowFailure::Other(reason) => reason,
            };
            asked.push(Action::ShowError(text));
        }
    }
    actions(asked)
}

fn discard_view_slot(ops: &mut Ops, slot: &Path) {
    if let Some(copies) = ops.view_copies.as_mut() {
        copies.discard(slot);
    }
}

/// Ends the bookkeeping of an unpacking whose job was stopped; its result
/// is ignored if it still arrives. The slot stays known: the unpacking
/// thread removes it when it notices the stop, and if it finished first,
/// the copy goes with the others of its archive.
pub fn end_extraction(ops: &mut Ops) {
    ops.bump_generation();
    ops.extracting = None;
}
