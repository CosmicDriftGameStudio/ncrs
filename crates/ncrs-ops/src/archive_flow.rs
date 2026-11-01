//! Writing an archive: a pack, a copy into one and a delete from one run as
//! the one job of the queue. Escape closes the progress channel, which makes
//! the writer drop its temporary file and leave the archive as it was.

use std::path::PathBuf;

use iced::Task;
use ncrs_core::PanelSide;
use ncrs_fs as fs;
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_jobs::{Job, JobKind};

use crate::action::{Action, StartProgressJob};
use crate::ctx::FlowCtx;
use crate::delete_flow::{actions, idle, Outcome};
use crate::message::OpsMessage;
use crate::row_failure::RowFailure;
use crate::state::{ArchiveWrite, ArchiveWriteKind, Deletion, Ops};
use crate::transfer::Policy;
use crate::transfer_flow::refusal_text;

/// What the pack prompt holds once its name passed validation, read by the app
/// from the prompt it closes.
#[derive(Debug)]
pub struct PackRequest {
    /// The archive to write.
    pub archive: PathBuf,
    pub sources: Vec<PathBuf>,
    /// The folder the entries keep their names below, when the pack keeps
    /// the folders of a hit list.
    pub keep_under: Option<PathBuf>,
    /// The panel the prompt belonged to.
    pub target_side: PanelSide,
}

/// The job behind Alt+F5: one job writes the archive.
pub fn start_pack(ops: &mut Ops, request: PackRequest, ctx: &FlowCtx<'_>) -> Outcome {
    let PackRequest {
        archive,
        sources,
        keep_under,
        target_side,
    } = request;
    let name = archive
        .file_name()
        .map(|file_name| file_name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let folder = archive
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();
    let write = ArchiveWrite {
        side: target_side.other(),
        what: ArchiveWriteKind::Pack { name, folder },
    };
    let packed = archive.clone();
    let numbered: Option<Vec<(PathBuf, String)>> = (keep_under.is_some()
        || fs::flat_names::from_several_folders(&sources))
    .then(|| fs::flat_names::archive_names(&sources, keep_under.as_deref()));
    start_archive_write(
        ops,
        JobKind::Pack,
        archive,
        write,
        move |progress| match numbered {
            Some(items) => fs::archive::pack_named(&items, &packed, progress),
            None => fs::archive::pack(&sources, &packed, progress),
        },
        ctx,
    )
}

/// F5 with an archive, or a folder in one, as the target: one job rewrites
/// the archive with the selection added. `Ask` cannot ask per file here, so
/// it stops at the first name in the way and says what the dialog offers.
pub fn begin_copy_into_archive(
    ops: &mut Ops,
    location: fs::archive::ArchiveLocation,
    sources: Vec<PathBuf>,
    policy: Policy,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    let write = ArchiveWrite {
        side: ctx.source_side,
        what: ArchiveWriteKind::CopyIn,
    };
    let conflict = policy.on_conflict();
    let fs::archive::ArchiveLocation { archive, inner, .. } = location;
    let shown = archive.clone();
    start_archive_write(
        ops,
        JobKind::Copy,
        shown,
        write,
        move |progress| fs::archive::add(&archive, &inner, &sources, conflict, progress),
        ctx,
    )
}

/// The delete dialog of a panel inside `archive` was confirmed.
pub fn start_archive_delete(
    ops: &mut Ops,
    archive: PathBuf,
    deletion: Deletion,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    let write = ArchiveWrite {
        side: deletion.side,
        what: ArchiveWriteKind::Delete,
    };
    let shown = archive.clone();
    start_archive_write(
        ops,
        JobKind::Delete,
        shown,
        write,
        move |progress| {
            let entries = deletion
                .sources
                .iter()
                .map(|path| {
                    fs::archive::locate_in_archive(path)
                        .filter(|place| place.archive == archive && !place.inner.is_empty())
                        .map(|place| place.inner)
                        .ok_or_else(|| {
                            std::io::Error::new(
                                std::io::ErrorKind::InvalidInput,
                                "the entry is not in this archive",
                            )
                        })
                })
                .collect::<std::io::Result<Vec<_>>>()?;
            fs::archive::delete(&archive, &entries, progress)
        },
        ctx,
    )
}

/// Runs `work` on the blocking pool as the one job of the queue. It reports
/// as `ArchiveWritten`.
fn start_archive_write(
    ops: &mut Ops,
    kind: JobKind,
    shown: PathBuf,
    write: ArchiveWrite,
    work: impl FnOnce(&mut fs::transfer::Reporting<'_>) -> std::io::Result<fs::archive::WriteReport>
        + Send
        + 'static,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    if ctx.jobs_busy
        || ops.transfer.is_some()
        || ops.deleting.is_some()
        || ops.extracting.is_some()
        || ops.archive_write.is_some()
    {
        let text = ctx.lang.text(Msg::StatusArchiveWriteBusy).to_owned();
        return actions(vec![Action::ShowNotice(text)]);
    }
    ops.bump_generation();
    let generation = ops.generation;
    ops.archive_write = Some(write);
    let job = Job {
        kind,
        path: shown,
        total: None,
    };
    let build = Box::new(move |progress_tx| {
        Task::perform(
            tokio::task::spawn_blocking(move || {
                let mut progress = fs::transfer::Reporting::new(&progress_tx);
                work(&mut progress)
            }),
            move |outcome| {
                let result = match outcome {
                    Ok(Ok(report)) => Ok(report),
                    Ok(Err(err)) => Err(RowFailure::from_io(err)),
                    Err(join) => Err(RowFailure::Other(join.to_string())),
                };
                OpsMessage::ArchiveWritten { result, generation }
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

pub fn archive_written(
    ops: &mut Ops,
    generation: u64,
    result: Result<fs::archive::WriteReport, RowFailure>,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    if generation != ops.generation {
        return idle();
    }
    let mut asked = vec![Action::FinishJob, Action::ClearJobProgress];
    let Some(write) = ops.archive_write.take() else {
        return actions(asked);
    };
    ops.bump_generation();
    let notice = match result {
        Ok(report) => Some(written_text(ctx.lang, &write, report)),
        Err(failure) => {
            asked.push(Action::ShowError(write_failure_text(
                ctx.lang, &write, failure,
            )));
            None
        }
    };
    asked.extend(end_archive_write(&write).1);
    if let Some(text) = notice {
        asked.push(Action::ShowNotice(text));
    }
    actions(asked)
}

fn written_text(lang: Language, write: &ArchiveWrite, report: fs::archive::WriteReport) -> String {
    let mut text = match &write.what {
        ArchiveWriteKind::Pack { name, .. } => {
            fill_template(lang.text(Msg::StatusPacked), &[("{name}", name)])
        }
        ArchiveWriteKind::CopyIn => lang.text(Msg::StatusArchiveCopiedIn).to_owned(),
        ArchiveWriteKind::Delete => lang.text(Msg::StatusArchiveDeleted).to_owned(),
    };
    for (count, template) in [
        (report.skipped, Msg::StatusArchiveSkipped),
        (report.kept_existing, Msg::StatusArchiveKept),
    ] {
        if count > 0 {
            text.push_str(&fill_template(
                lang.text(template),
                &[("{count}", &count.to_string())],
            ));
        }
    }
    text
}

fn write_failure_text(lang: Language, write: &ArchiveWrite, failure: RowFailure) -> String {
    match failure {
        RowFailure::Refused(refusal) => refusal_text(lang, refusal),
        RowFailure::Conflict(detail) if write.what == ArchiveWriteKind::CopyIn => fill_template(
            lang.text(Msg::ErrorArchiveNameTaken),
            &[("{detail}", &detail)],
        ),
        RowFailure::Conflict(reason)
        | RowFailure::UnsafeName(reason)
        | RowFailure::Other(reason) => fill_template(
            lang.text(Msg::ErrorArchiveWriteFailed),
            &[("{reason}", &reason)],
        ),
    }
}

/// Stops the write whose job was aborted. `None` when none runs.
pub fn abort_archive_write(ops: &mut Ops) -> Option<Outcome> {
    let write = ops.archive_write.take()?;
    ops.bump_generation();
    Some(end_archive_write(&write))
}

/// Ends the bookkeeping of a write that finished, failed or was stopped:
/// the tags of a delete refer to entries that may be gone, and both panels
/// are read again, the new archive of a pack selected.
fn end_archive_write(write: &ArchiveWrite) -> Outcome {
    let mut asked = Vec::new();
    if write.what == ArchiveWriteKind::Delete {
        asked.push(Action::ClearSelection(write.side));
    }
    asked.push(match &write.what {
        ArchiveWriteKind::Pack { name, folder } => Action::ReloadPanelsSelecting {
            folder: folder.clone(),
            name: name.clone(),
        },
        ArchiveWriteKind::CopyIn | ArchiveWriteKind::Delete => Action::ReloadPanels,
    });
    asked.push(Action::RestartSearch);
    actions(asked)
}
