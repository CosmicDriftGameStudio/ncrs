//! The copy and move: one row per job, so a name that is taken can be
//! answered between two files and Escape works between two.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use iced::Task;
use ncrs_core::TransferKind;
use ncrs_fs as fs;
use ncrs_fs::remote::RemotePath;
use ncrs_fs::transfer::OnConflict;
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_jobs::{Job, JobKind};

use crate::action::{Action, StartProgressJob};
use crate::archive_flow::begin_copy_into_archive;
use crate::conflict::OVERWRITE_FOCUS;
use crate::ctx::FlowCtx;
use crate::delete_flow::{actions, idle, with_front, Outcome};
use crate::message::{ConflictChoice, OpsMessage};
use crate::restore_flow::resume_restore;
use crate::row_failure::RowFailure;
use crate::state::{Ops, PendingConflict, Transfer};
use crate::transfer::Policy;

/// `name` in `folder`. A folder on a server is joined as text, because
/// `Path::join` would put a backslash in on Windows.
pub fn child_path(folder: &Path, name: &OsStr) -> PathBuf {
    match (RemotePath::parse(folder), name.to_str()) {
        (Some(place), Some(plain)) => place.join(plain).to_path_buf(),
        _ => folder.join(name),
    }
}

fn job_kind(kind: TransferKind) -> JobKind {
    match kind {
        TransferKind::Copy => JobKind::Copy,
        TransferKind::Move => JobKind::Move,
    }
}

/// The words for a 7z archive ncrs refuses to unpack.
pub fn refusal_text(lang: Language, refusal: fs::archive::SevenZipRefusal) -> String {
    lang.text(match refusal {
        fs::archive::SevenZipRefusal::Encrypted => Msg::ErrorArchiveEncrypted,
        fs::archive::SevenZipRefusal::DictionaryTooLarge => Msg::ErrorArchiveDictionaryTooLarge,
    })
    .to_owned()
}

/// Starts a transfer whose target passed `transfer_dialog::validate`.
pub fn start_validated_transfer(
    ops: &mut Ops,
    kind: TransferKind,
    sources: Vec<PathBuf>,
    target: PathBuf,
    policy: Policy,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    let needs_numbering = fs::flat_names::from_several_folders(&sources)
        && fs::flat_names::flat_names(&sources)
            .iter()
            .zip(&sources)
            .any(|(name, source)| name.as_deref() != source.file_name());
    let into_archive = fs::archive::locate_in_archive(&target);
    if needs_numbering && (into_archive.is_some() || RemotePath::parse(&target).is_some()) {
        let text = ctx.lang.text(Msg::StatusNumberingNeedsFolder).to_owned();
        return actions(vec![Action::ShowError(text)]);
    }
    if let Some(location) = into_archive {
        return begin_copy_into_archive(ops, location, sources, policy, ctx);
    }
    begin_transfer(ops, kind, sources, target, policy, ctx)
}

/// Starts the first row of a confirmed transfer and shows its progress.
pub fn begin_transfer(
    ops: &mut Ops,
    kind: TransferKind,
    sources: Vec<PathBuf>,
    target: PathBuf,
    policy: Policy,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    let names = if fs::flat_names::from_several_folders(&sources) {
        fs::flat_names::flat_names(&sources)
            .into_iter()
            .map(Option::unwrap_or_default)
            .collect()
    } else {
        Vec::new()
    };
    let transfer = Transfer {
        kind,
        sources,
        target,
        names,
    };
    ops.transfer = Some(transfer.clone());
    ops.conflict_rule = policy.standing_rule();
    ops.bump_generation();
    ops.transfer_mask_visible = true;
    ops.transfer_rows_done = 0;
    run_from(ops, transfer, 0, policy.on_conflict(), ctx)
}

/// Runs the rows from `index`, on the blocking pool, reporting progress.
///
/// One row at a time rather than the whole list, so a conflict can be
/// answered between two files and the queue stays one job.
fn run_from(
    ops: &mut Ops,
    transfer: Transfer,
    index: usize,
    conflict: OnConflict,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    let Some(source) = transfer.sources.get(index).cloned() else {
        return end_transfer(ops);
    };

    let generation = ops.generation;
    let target = transfer.target.clone();
    let name_in_target = transfer.name_in_target(index);
    let pool = Arc::clone(ctx.remote);
    let job = Job {
        kind: job_kind(transfer.kind),
        path: child_path(
            &target,
            name_in_target
                .as_deref()
                .unwrap_or_else(|| source.file_name().unwrap_or_default()),
        ),
        total: None,
    };
    // One row per job keeps a conflict answerable between two files.
    let build = Box::new(move |progress_tx| {
        // transfer::run is blocking filesystem work, so it goes on the
        // blocking pool — the same reason read_directory does.
        Task::perform(
            tokio::task::spawn_blocking({
                let from = source.clone();
                let to = target.clone();
                let kind = transfer.kind;
                let renamed = name_in_target.clone();
                move || {
                    if let Some(name) = renamed {
                        return fs::transfer::run_reporting_as(
                            &from,
                            &to.join(name),
                            kind,
                            conflict,
                            &progress_tx,
                        );
                    }
                    // Reports as it goes, so the bar moves during a long
                    // copy rather than jumping at the end. A server on
                    // either side is the SFTP layer's.
                    fs::remote::transfer::run_reporting(
                        &pool,
                        &from,
                        &to,
                        kind,
                        conflict,
                        &progress_tx,
                    )
                    .unwrap_or_else(|| {
                        fs::transfer::run_reporting(&from, &to, kind, conflict, &progress_tx)
                    })
                }
            }),
            move |outcome| {
                // Two failures collapse into one string here: the join error
                // means the blocking task died, the other means the copy
                // itself failed. The dialog cannot tell them apart anyway.
                let result = match outcome {
                    Ok(Ok(_)) => Ok(transfer.clone()),
                    // The kind travels with the message rather than being
                    // formatted into it: the dialog has to recognise a
                    // conflict, and it cannot do that from wording.
                    Ok(Err(e)) => Err(RowFailure::from_io(e)),
                    // A dead blocking task is not a conflict. Its reason is
                    // a string already, so it is shown as one.
                    Err(join) => Err(RowFailure::Other(join.to_string())),
                };
                OpsMessage::TransferRowDone {
                    result,
                    index,
                    generation,
                }
            },
        )
    });
    actions(vec![Action::StartJobWithProgress(StartProgressJob {
        job,
        build,
    })])
}

/// One row of a transfer finished. Continue with the next, or ask about a
/// name that is in the way.
pub fn transfer_row_done(
    ops: &mut Ops,
    result: Result<Transfer, RowFailure>,
    index: usize,
    generation: u64,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    if generation != ops.generation {
        return idle();
    }
    let finished = vec![Action::FinishJob, Action::CountJobDone];
    let Some(transfer) = ops.transfer.clone() else {
        return actions(finished);
    };
    let (task, rest) = match result {
        // A remembered "all" answer applies to the rows after this one,
        // without asking again.
        Ok(done) => {
            ops.transfer_rows_done = index + 1;
            let rule = ops.conflict_rule.unwrap_or(OnConflict::Fail);
            run_from(ops, done, index + 1, rule, ctx)
        }
        // A name that is taken is the one the user can answer. Matched on
        // the error kind, not on the message text: a wording change in the
        // filesystem layer would otherwise silently turn every conflict into
        // a hard failure. The dialog names the file itself, so the reason
        // travels no further; it stays in the payload for the status line.
        Err(RowFailure::Conflict(_reason)) => {
            ops.pending_conflict = Some(PendingConflict { transfer, index });
            actions(vec![
                Action::FocusDialogButton(OVERWRITE_FOCUS),
                // Else the dialog would be asked behind the box.
                Action::CloseQuickOpenAndPalette,
            ])
        }
        // Rows before this one are already done, so the panels are reloaded
        // to show them.
        Err(RowFailure::Refused(refusal)) => with_front(
            Action::ShowError(refusal_text(ctx.lang, refusal)),
            end_transfer(ops),
        ),
        Err(RowFailure::UnsafeName(name)) => {
            let text = fill_template(
                ctx.lang.text(Msg::ErrorRemoteUnsafeName),
                &[("{name}", &name)],
            );
            with_front(Action::ShowError(text), end_transfer(ops))
        }
        Err(RowFailure::Other(reason)) => with_front(Action::ShowError(reason), end_transfer(ops)),
    };
    let mut asked = finished;
    asked.extend(rest);
    (task, asked)
}

/// Answers a conflict and remembers an "all" answer, so the next conflict
/// does not ask again.
pub fn answer_conflict(ops: &mut Ops, clicked: ConflictChoice, ctx: &FlowCtx<'_>) -> Outcome {
    if let Some(index) = ops.restore_conflict.take() {
        let choice = ops.conflict_answer(clicked);
        return resume_restore(ops, index, choice, ctx);
    }
    let Some(pending) = ops.pending_conflict.take() else {
        return idle();
    };
    let choice = ops.conflict_answer(clicked);
    // Cancel ends the transfer; the rows already done stay done.
    if choice == ConflictChoice::Cancel {
        return end_transfer(ops);
    }
    if choice.applies_to_all() {
        ops.conflict_rule = Some(choice.conflict());
    }
    // The task is returned, not dropped: dropping it would leave the
    // transfer standing still after the user answered, which is the one
    // thing the dialog must not do.
    let PendingConflict { transfer, index } = pending;
    ops.transfer = Some(transfer.clone());
    run_from(ops, transfer, index, choice.conflict(), ctx)
}

/// Ends the transfer, finished or not: forgets it and its "for all" rule, and
/// reloads both panels, since a move changes the source directory as well as
/// the target and rows before a failure or a cancel are already done.
pub fn end_transfer(ops: &mut Ops) -> Outcome {
    ops.transfer = None;
    ops.transfer_mask_visible = false;
    ops.transfer_rows_done = 0;
    ops.conflict_rule = None;
    ops.bump_generation();
    actions(vec![Action::ReloadSourceThenTarget, Action::RestartSearch])
}
