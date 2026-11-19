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
use crate::conflict::KEEP_FOCUS;
use crate::ctx::FlowCtx;
use crate::delete_flow::{actions, busy_notice, idle, with_front, Outcome};
use crate::message::{ConflictChoice, OpsMessage};
use crate::restore_flow::resume_restore;
use crate::row_failure::RowFailure;
use crate::skipped_report::skipped_text;
use crate::state::{Ops, PendingConflict, Transfer};
use crate::transfer::view::BACKGROUND_FOCUS;
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
    ops.transfer_completed = false;
    let needs_numbering = fs::flat_names::from_several_folders(&sources)
        && fs::flat_names::flat_names(&sources)
            .iter()
            .zip(&sources)
            .any(|(name, source)| name.as_deref() != source.file_name());
    if ctx.jobs_busy {
        return busy_notice(ctx);
    }
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
        targets: Vec::new(),
    };
    launch(ops, transfer, policy, ctx)
}

/// Copies each source into its own folder, as a sync wants. Files that come
/// from several folders keep their names, which numbering would not.
pub fn start_rows_transfer(
    ops: &mut Ops,
    rows: Vec<(PathBuf, PathBuf)>,
    policy: Policy,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    ops.transfer_completed = false;
    let Some(first_target) = rows.first().map(|(_, target)| target.clone()) else {
        return idle();
    };
    let (sources, targets): (Vec<_>, Vec<_>) = rows.into_iter().unzip();
    let one_target = targets.iter().all(|target| *target == first_target);
    if one_target && !fs::flat_names::from_several_folders(&sources) {
        return start_validated_transfer(
            ops,
            TransferKind::Copy,
            sources,
            first_target,
            policy,
            ctx,
        );
    }
    if ctx.jobs_busy {
        return busy_notice(ctx);
    }
    let transfer = Transfer {
        kind: TransferKind::Copy,
        sources,
        target: first_target,
        names: Vec::new(),
        targets,
    };
    launch(ops, transfer, policy, ctx)
}

fn launch(ops: &mut Ops, transfer: Transfer, policy: Policy, ctx: &FlowCtx<'_>) -> Outcome {
    ops.transfer_completed = false;
    ops.transfer_skipped.clear();
    ops.transfer = Some(transfer.clone());
    ops.conflict_rule = policy.standing_rule();
    ops.bump_generation();
    ops.transfer_mask_visible = true;
    ops.transfer_rows_done = 0;
    with_front(
        Action::FocusDialogButton(BACKGROUND_FOCUS),
        run_from(ops, transfer, 0, policy.on_conflict(), ctx),
    )
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
        ops.transfer_completed = true;
        return end_transfer(ops, ctx.lang);
    };

    let generation = ops.generation;
    let target = transfer.target_of(index).to_path_buf();
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
                    // Reports as it goes, so the bar moves during a long
                    // copy rather than jumping at the end.
                    let mut report = fs::transfer::Reporting::new(&progress_tx);
                    let result = match renamed {
                        Some(name) => fs::transfer::run_as_with(
                            &from,
                            &to.join(name),
                            kind,
                            conflict,
                            &mut report,
                        ),
                        // A server on either side is the SFTP layer's.
                        None => fs::remote::transfer::run_with(
                            &pool,
                            &from,
                            &to,
                            kind,
                            conflict,
                            &mut report,
                        )
                        .unwrap_or_else(|| {
                            fs::transfer::run_with(&from, &to, kind, conflict, &mut report)
                        }),
                    };
                    (result, report.take_skipped())
                }
            }),
            move |joined| {
                // Two failures collapse into one string here: the join error
                // means the blocking task died, the other means the copy
                // itself failed. The dialog cannot tell them apart anyway.
                let (outcome, skipped) = match joined {
                    Ok((result, skipped)) => (Ok(result), skipped),
                    Err(join) => (Err(join), Vec::new()),
                };
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
                    skipped,
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
    skipped: Vec<fs::transfer::Skipped>,
    index: usize,
    generation: u64,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    if generation != ops.generation {
        return idle();
    }
    for item in skipped {
        if !ops.transfer_skipped.contains(&item) {
            ops.transfer_skipped.push(item);
        }
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
            ops.conflict_folder = folder_onto_folder(&transfer, index);
            ops.pending_conflict = Some(PendingConflict { transfer, index });
            actions(vec![
                Action::FocusDialogButton(KEEP_FOCUS),
                // Else the dialog would be asked behind the box.
                Action::CloseQuickOpenAndPalette,
            ])
        }
        // Rows before this one are already done, so the panels are reloaded
        // to show them.
        Err(RowFailure::Refused(refusal)) => with_front(
            Action::ShowError(refusal_text(ctx.lang, refusal)),
            end_transfer(ops, ctx.lang),
        ),
        Err(RowFailure::UnsafeName(name)) => {
            let text = fill_template(
                ctx.lang.text(Msg::ErrorRemoteUnsafeName),
                &[("{name}", &name)],
            );
            with_front(Action::ShowError(text), end_transfer(ops, ctx.lang))
        }
        Err(failure @ (RowFailure::Io { .. } | RowFailure::Other(_))) => with_front(
            Action::ShowError(failure.reason_text(ctx.lang)),
            end_transfer(ops, ctx.lang),
        ),
    };
    let mut asked = finished;
    asked.extend(rest);
    (task, asked)
}

/// Whether the row at `index` is a folder going onto a folder, which an
/// overwrite merges. A server's folders are not looked at here.
fn folder_onto_folder(transfer: &Transfer, index: usize) -> bool {
    let Some(source) = transfer.sources.get(index) else {
        return false;
    };
    if RemotePath::parse(source).is_some() || RemotePath::parse(transfer.target_of(index)).is_some()
    {
        return false;
    }
    let name = transfer
        .name_in_target(index)
        .or_else(|| source.file_name().map(OsStr::to_os_string))
        .unwrap_or_default();
    // Links are not followed: a link to a folder is a file here, and a merge
    // through it would write into whatever it points at.
    let is_folder = |path: &Path| std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir());
    is_folder(source) && is_folder(&transfer.target_of(index).join(name))
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
        return end_transfer(ops, ctx.lang);
    }
    if choice.applies_to_all() {
        ops.conflict_rule = Some(choice.conflict());
    }
    // The task is returned, not dropped: dropping it would leave the
    // transfer standing still after the user answered, which is the one
    // thing the dialog must not do.
    let PendingConflict { transfer, index } = pending;
    ops.transfer = Some(transfer.clone());
    with_front(
        Action::FocusDialogButton(BACKGROUND_FOCUS),
        run_from(ops, transfer, index, choice.conflict(), ctx),
    )
}

/// Ends the transfer, finished or not: forgets it and its "for all" rule, and
/// reloads both panels, since a move changes the source directory as well as
/// the target and rows before a failure or a cancel are already done.
pub fn end_transfer(ops: &mut Ops, lang: Language) -> Outcome {
    ops.transfer = None;
    // A pasted clipboard file was staged only to be copied; it must not outlive the copy.
    ops.paste_staging = None;
    ops.transfer_mask_visible = false;
    ops.transfer_rows_done = 0;
    ops.conflict_rule = None;
    ops.bump_generation();
    let skipped = std::mem::take(&mut ops.transfer_skipped);
    let mut asked = vec![Action::ReloadSourceThenTarget, Action::RestartSearch];
    asked.extend(skipped_text(lang, &skipped).map(Action::ShowNotice));
    actions(asked)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use ncrs_core::PanelSide;
    use ncrs_fs::remote::session::Pool;
    use ncrs_fs::remote::KnownHostsFiles;

    fn with_ctx<R>(jobs_busy: bool, step: impl FnOnce(&FlowCtx<'_>) -> R) -> R {
        let trash = fs::system_trash(None);
        let sandbox = fs::SandboxAccess::inactive();
        let remote = Arc::new(Pool::over_tcp(KnownHostsFiles::default()));
        step(&FlowCtx {
            lang: Language::default(),
            trash: &trash,
            jobs_busy,
            start_blocked: false,
            transfer_dialog_open: false,
            has_job_error: false,
            sandbox: &sandbox,
            source_side: PanelSide::Left,
            left_archive: None,
            right_archive: None,
            left_server: None,
            right_server: None,
            remote: &remote,
            permanent_delete_key: None,
            restore_last_key: None,
            index: None,
            model: None,
        })
    }

    fn transfer_of(source: PathBuf, target: PathBuf) -> Transfer {
        Transfer {
            kind: TransferKind::Copy,
            sources: vec![source],
            target,
            names: Vec::new(),
            targets: Vec::new(),
        }
    }

    #[test]
    fn a_taken_folder_asks_with_keep_in_focus_and_offers_to_merge() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        std::fs::create_dir(from.path().join("photos")).unwrap();
        std::fs::create_dir(to.path().join("photos")).unwrap();
        let transfer = transfer_of(from.path().join("photos"), to.path().to_owned());
        let mut ops = Ops {
            transfer: Some(transfer),
            ..Ops::default()
        };

        let generation = ops.generation;
        let (_task, asked) = with_ctx(false, |ctx| {
            transfer_row_done(
                &mut ops,
                Err(RowFailure::Conflict("taken".to_owned())),
                Vec::new(),
                0,
                generation,
                ctx,
            )
        });

        assert!(ops.conflict_folder);
        assert!(asked.iter().any(
            |action| matches!(action, Action::FocusDialogButton(button) if *button == KEEP_FOCUS)
        ));
    }

    #[test]
    fn a_taken_file_is_not_a_merge() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        std::fs::write(from.path().join("a.txt"), "new").unwrap();
        std::fs::write(to.path().join("a.txt"), "old").unwrap();
        let transfer = transfer_of(from.path().join("a.txt"), to.path().to_owned());

        assert!(!folder_onto_folder(&transfer, 0));
    }

    #[test]
    fn rows_go_each_into_their_own_folder() {
        let transfer = Transfer {
            targets: vec![PathBuf::from("/b/deep"), PathBuf::from("/b")],
            ..transfer_of(PathBuf::from("/a/deep/x"), PathBuf::from("/b/deep"))
        };

        assert_eq!(transfer.target_of(0), Path::new("/b/deep"));
        assert_eq!(transfer.target_of(1), Path::new("/b"));
    }

    #[test]
    fn a_transfer_asked_for_while_a_job_runs_is_refused_and_not_queued() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        let mut ops = Ops::default();

        let (_task, asked) = with_ctx(true, |ctx| {
            start_validated_transfer(
                &mut ops,
                TransferKind::Copy,
                vec![from.path().join("a.txt")],
                to.path().to_owned(),
                Policy::Ask,
                ctx,
            )
        });

        assert!(ops.transfer.is_none());
        assert!(matches!(asked.as_slice(), [Action::ShowNotice(_)]));
    }

    #[test]
    fn a_refused_transfer_does_not_leave_the_last_completion_standing() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        let mut ops = Ops {
            transfer_completed: true,
            ..Ops::default()
        };

        drop(with_ctx(true, |ctx| {
            start_validated_transfer(
                &mut ops,
                TransferKind::Copy,
                vec![from.path().join("a.txt")],
                to.path().to_owned(),
                Policy::Ask,
                ctx,
            )
        }));
        assert!(!ops.transfer_completed);

        ops.transfer_completed = true;
        drop(with_ctx(true, |ctx| {
            start_rows_transfer(
                &mut ops,
                vec![
                    (from.path().join("a.txt"), to.path().join("x")),
                    (from.path().join("b.txt"), to.path().join("y")),
                ],
                Policy::Ask,
                ctx,
            )
        }));
        assert!(!ops.transfer_completed);
    }

    #[test]
    fn what_a_finished_transfer_left_out_is_reported_at_its_end() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        let transfer = transfer_of(from.path().join("photos"), to.path().to_owned());
        let mut ops = Ops {
            transfer: Some(transfer.clone()),
            ..Ops::default()
        };
        let skipped = vec![fs::transfer::Skipped {
            path: from.path().join("photos/socket"),
            kind: fs::transfer::SkipKind::Special,
            detail: String::new(),
        }];
        let generation = ops.generation;

        let (_task, asked) = with_ctx(false, |ctx| {
            transfer_row_done(&mut ops, Ok(transfer), skipped, 0, generation, ctx)
        });

        assert!(asked
            .iter()
            .any(|action| matches!(action, Action::ShowNotice(text) if text.contains("socket"))));
        assert!(ops.transfer_skipped.is_empty());
    }
    #[test]
    fn the_staged_paste_is_removed_when_the_transfer_ends() {
        let staging = tempfile::tempdir().unwrap();
        let staged = staging.path().to_owned();
        std::fs::write(staged.join("pasted.txt"), b"secret").unwrap();
        let mut ops = Ops {
            paste_staging: Some(staging),
            ..Ops::default()
        };

        drop(end_transfer(&mut ops, Language::default()));

        assert!(!staged.exists());
    }
    #[test]
    fn a_finished_copy_into_an_archive_counts_as_completed() {
        let mut ops = Ops {
            archive_write: Some(crate::state::ArchiveWrite {
                side: PanelSide::Left,
                archive: PathBuf::from("/x/a.zip"),
                what: crate::state::ArchiveWriteKind::CopyIn,
            }),
            ..Ops::default()
        };
        let generation = ops.generation;

        drop(with_ctx(false, |ctx| {
            crate::archive_flow::archive_written(
                &mut ops,
                generation,
                Ok(fs::archive::WriteReport::default()),
                ctx,
            )
        }));

        assert!(ops.transfer_completed);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_a_folder_is_not_merged_into() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        std::fs::create_dir(from.path().join("photos")).unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), to.path().join("photos")).unwrap();
        let transfer = transfer_of(from.path().join("photos"), to.path().to_owned());

        assert!(!folder_onto_folder(&transfer, 0));
    }
}
