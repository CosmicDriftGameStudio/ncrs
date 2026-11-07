//! Changing permissions: the dialog, then one background job that reports
//! its progress like a copy does.

use iced::Task;
use ncrs_fs::permissions::{change_permissions, Outcome as Changes};
use ncrs_i18n::{fill_template, Msg};
use ncrs_jobs::{Job, JobKind};
use ncrs_ui_kit::focus::release_focus;

use crate::action::{Action, ProgressJobBuilder, StartProgressJob};
use crate::ctx::FlowCtx;
use crate::delete_flow::{actions, idle, Outcome};
use crate::message::{OpsMessage, PermissionsMessage};
use crate::permissions::{PermissionTarget, PermissionsDialog};
use crate::state::Ops;

/// The rights key or the menu entry: ask before anything is changed. Nothing
/// to act on, or a job or another dialog in the way, means nothing happens.
pub fn open_permissions(
    ops: &mut Ops,
    targets: Vec<PermissionTarget>,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    if ctx.start_blocked || ops.permissions_dialog.is_some() || targets.is_empty() {
        return idle();
    }
    ops.permissions_dialog = Some(PermissionsDialog::new(targets, cfg!(windows)));
    actions(vec![Action::ClearStatus])
}

/// A message of the dialog.
pub fn update_permissions(ops: &mut Ops, message: PermissionsMessage) -> Outcome {
    let Some(dialog) = ops.permissions_dialog.as_mut() else {
        return idle();
    };
    match message {
        PermissionsMessage::Toggle(index) => dialog.click(index),
        PermissionsMessage::OctalInput(text) => dialog.type_octal(text),
        PermissionsMessage::ToggleAdvanced => dialog.toggle_advanced(),
        PermissionsMessage::ToggleContents => dialog.toggle_contents(),
        PermissionsMessage::SetForFiles(choice) => dialog.set_for_files(choice),
        PermissionsMessage::SetForFolders(choice) => dialog.set_for_folders(choice),
        PermissionsMessage::Cancel => {
            ops.permissions_dialog = None;
            return (release_focus(), Vec::new());
        }
        PermissionsMessage::Confirm => return confirm(ops),
    }
    idle()
}

/// Apply: close the dialog and start the job, or only close when nothing was
/// touched.
fn confirm(ops: &mut Ops) -> Outcome {
    let Some(dialog) = ops.permissions_dialog.take() else {
        return idle();
    };
    let Some(plan) = dialog.plan() else {
        return (release_focus(), Vec::new());
    };
    let chosen = dialog.paths();
    ops.permissions_running = true;
    ops.bump_generation();
    let generation = ops.generation;
    let job = Job {
        kind: JobKind::Permissions,
        path: chosen
            .first()
            .and_then(|first| first.parent())
            .map(std::path::Path::to_path_buf)
            .unwrap_or_default(),
        total: None,
    };
    let build: ProgressJobBuilder = Box::new(move |progress_tx| {
        Task::perform(
            tokio::task::spawn_blocking(move || change_permissions(&chosen, &plan, &progress_tx)),
            move |joined| OpsMessage::PermissionsDone {
                generation,
                result: joined.map_err(|failure| failure.to_string()),
            },
        )
    });
    (
        release_focus(),
        vec![
            Action::ClearStatus,
            Action::ResetJobDone,
            Action::ClearJobProgress,
            Action::StartJobWithProgress(StartProgressJob { job, build }),
        ],
    )
}

/// The job is over. A result of a job that was aborted meanwhile is dropped.
pub fn permissions_done(
    ops: &mut Ops,
    generation: u64,
    result: Result<Changes, String>,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    if generation != ops.generation || !ops.permissions_running {
        return idle();
    }
    ops.permissions_running = false;
    ops.bump_generation();
    let mut asked = vec![Action::FinishJob, Action::ClearJobProgress];
    match result {
        Ok(changes) => match changes.failures.first() {
            None => asked.push(Action::ShowNotice(fill_template(
                ctx.lang.text(Msg::PermissionsDone),
                &[("{count}", &changes.changed.to_string())],
            ))),
            Some(first) => asked.push(Action::ShowError(fill_template(
                ctx.lang.text(Msg::PermissionsFailed),
                &[
                    ("{count}", &changes.failures.len().to_string()),
                    ("{reason}", &first.reason),
                ],
            ))),
        },
        Err(reason) => asked.push(Action::ShowError(reason)),
    }
    asked.extend([Action::ReloadPanels, Action::RestartSearch]);
    actions(asked)
}

/// Stops the change whose job was aborted. `None` when none runs. Entries
/// already changed stay changed, so the panels are read again.
pub fn abort_permissions(ops: &mut Ops) -> Option<Outcome> {
    if !std::mem::take(&mut ops.permissions_running) {
        return None;
    }
    ops.bump_generation();
    Some(actions(vec![Action::ReloadPanels, Action::RestartSearch]))
}
