//! What Enter asks of the file system before a job may start, run on the
//! blocking pool so a slow mount cannot freeze the window: the check of a
//! transfer target, of the archive a pack would write, and the write of a
//! pasted picture or text. The answers come back as [`OpsMessage`]s and go
//! through [`update_checked`].

use std::path::PathBuf;

use iced::Task;
use ncrs_core::TransferKind;
use ncrs_i18n::Msg;
use ncrs_ui_kit::focus::release_focus;

use crate::action::Action;
use crate::archive_flow::start_pack;
use crate::clipboard_paste::PastedContent;
use crate::ctx::FlowCtx;
use crate::delete_flow::{actions, busy_notice, idle, with_front, Outcome};
use crate::message::OpsMessage;
use crate::prompt::{PromptKind, PromptProblem};
use crate::state::Ops;
use crate::transfer::view::refusal_text;
use crate::transfer::{self, Policy, Refusal};
use crate::transfer_flow::start_validated_transfer;
use crate::transfer_open::unreachable_server_text;

/// Runs `work` on the blocking pool; a panic in it reads as `on_panic`.
fn off_the_ui_thread<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    on_panic: T,
    deliver: impl Fn(T) -> OpsMessage + Send + 'static,
) -> Task<OpsMessage> {
    Task::perform(tokio::task::spawn_blocking(work), move |done| {
        deliver(done.unwrap_or(on_panic))
    })
}

/// Enter in the copy and move dialog: checks the typed target against the
/// selection. [`transfer_checked`] starts the transfer or shows the refusal.
pub fn check_transfer_dialog(ops: &Ops) -> Outcome {
    let Some(dialog) = ops.transfer_dialog.as_ref() else {
        return idle();
    };
    let (id, edit, kind) = (dialog.id(), dialog.edit(), dialog.kind());
    let (sources, requested) = (dialog.sources().to_vec(), dialog.target_path());
    let check = off_the_ui_thread(
        move || transfer::validate(kind, &sources, &requested),
        Err(Refusal::Unreadable),
        move |result| OpsMessage::TransferChecked {
            dialog: id,
            edit,
            kind,
            result,
        },
    );
    (check, Vec::new())
}

/// The check of the copy and move dialog is back. An answer for a dialog that
/// was closed or edited meanwhile is dropped.
pub fn transfer_checked(
    ops: &mut Ops,
    (dialog, edit, kind): (u64, u64, TransferKind),
    result: Result<PathBuf, Refusal>,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    let Some(open) = ops
        .transfer_dialog
        .as_mut()
        .filter(|open| open.id() == dialog && open.edit() == edit && open.kind() == kind)
    else {
        return idle();
    };
    let target = match result {
        Ok(target) => target,
        Err(refusal) => {
            open.refuse(refusal);
            return idle();
        }
    };
    let sources = open.sources().to_vec();
    let policy = open.policy();
    if let Some(refusal) = unreachable_server_text(ctx.remote, &target, ctx.lang) {
        return actions(vec![Action::ShowError(refusal)]);
    }
    if ctx.jobs_busy {
        return busy_notice(ctx);
    }
    ops.transfer_dialog = None;
    let (started, asked) = start_validated_transfer(ops, kind, sources, target, policy, ctx);
    (Task::batch([release_focus(), started]), asked)
}

/// Checks `paths` for a copy into `target`; [`external_checked`] starts it.
pub fn check_external_copy(paths: Vec<PathBuf>, target: PathBuf) -> Task<OpsMessage> {
    let checked = paths.clone();
    off_the_ui_thread(
        move || transfer::validate(TransferKind::Copy, &checked, &target),
        Err(Refusal::Unreadable),
        move |result| OpsMessage::ExternalChecked {
            paths: paths.clone(),
            result,
        },
    )
}

/// The check of files that came from outside the panels is back.
pub fn external_checked(
    ops: &mut Ops,
    paths: Vec<PathBuf>,
    result: Result<PathBuf, Refusal>,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    if ops.transfer_into_panel_is_blocked(ops.transfer_dialog.is_some()) {
        ops.paste_staging = None;
        let text = ctx.lang.text(Msg::StatusDropBusy).to_owned();
        return actions(vec![Action::ShowNotice(text)]);
    }
    let resolved = match result {
        Ok(resolved) => resolved,
        Err(refusal) => {
            ops.paste_staging = None;
            let text = refusal_text(refusal, ctx.lang).to_owned();
            return actions(vec![Action::ShowError(text)]);
        }
    };
    if let Some(unreachable) = unreachable_server_text(ctx.remote, &resolved, ctx.lang) {
        ops.paste_staging = None;
        return actions(vec![Action::ShowError(unreachable)]);
    }
    with_front(
        Action::ClearStatus,
        start_validated_transfer(ops, TransferKind::Copy, paths, resolved, Policy::Ask, ctx),
    )
}

/// Enter in the pack prompt, once its name passed the checks that need no
/// disk: asks the disk about the archive to write.
pub fn check_pack_target(ops: &mut Ops, ctx: &FlowCtx<'_>) -> Outcome {
    if ctx.jobs_busy {
        return busy_notice(ctx);
    }
    let Some(prompt) = ops.prompt.as_ref().filter(|prompt| !prompt.busy()) else {
        return idle();
    };
    let check = prompt.pack_target_check();
    let request_id = ops.prompt_request_id.wrapping_add(1);
    ops.prompt_request_id = request_id;
    ops.set_prompt_busy(true);
    let task = off_the_ui_thread(
        move || check.run(),
        Err(PromptProblem::ArchiveFolderMissing),
        move |result| OpsMessage::PackChecked { request_id, result },
    );
    (task, Vec::new())
}

/// The check of the archive a pack would write is back.
pub fn pack_checked(
    ops: &mut Ops,
    request_id: u64,
    result: Result<(), PromptProblem>,
    ctx: &FlowCtx<'_>,
) -> Outcome {
    let is_open_pack_prompt = ops
        .prompt
        .as_ref()
        .is_some_and(|prompt| prompt.kind() == PromptKind::Pack);
    if !ops.is_current_prompt_request(request_id) || !is_open_pack_prompt {
        return idle();
    }
    ops.set_prompt_busy(false);
    if let Err(problem) = result {
        if let Some(prompt) = ops.prompt.as_mut() {
            prompt.set_validation_error(problem);
        }
        return idle();
    }
    if ctx.jobs_busy {
        return busy_notice(ctx);
    }
    let Some(request) = ops.take_pack_request() else {
        return idle();
    };
    start_pack(ops, request, ctx)
}

/// Enter in the paste prompt: writes the picture or text under the name
/// typed into a private folder. [`paste_staged`] copies the file on.
pub fn stage_paste(ops: &mut Ops) -> Outcome {
    let Some((prompt, pasted_bytes)) = ops.prompt.as_ref().zip(
        ops.pending_paste
            .as_ref()
            .and_then(PastedContent::file_bytes),
    ) else {
        return idle();
    };
    if prompt.busy() {
        return idle();
    }
    let (name, bytes) = (prompt.name().trim().to_owned(), pasted_bytes.to_vec());
    let folder = match tempfile::Builder::new().prefix("ncrs-paste-").tempdir() {
        Ok(folder) => folder,
        Err(error) => {
            ops.set_prompt_error_text(&error.to_string());
            return idle();
        }
    };
    let file = folder.path().join(name);
    ops.paste_staging = Some(folder);
    ops.set_prompt_busy(true);
    let request_id = ops.prompt_request_id.wrapping_add(1);
    ops.prompt_request_id = request_id;
    let task = off_the_ui_thread(
        move || {
            std::fs::write(&file, bytes)
                .map(|()| file)
                .map_err(|error| error.to_string())
        },
        Err(String::new()),
        move |result| OpsMessage::PasteStaged { request_id, result },
    );
    (task, Vec::new())
}

/// The pasted picture or text is written, or writing it failed.
pub fn paste_staged(ops: &mut Ops, request_id: u64, result: Result<PathBuf, String>) -> Outcome {
    if !ops.is_current_prompt_request(request_id) {
        return idle();
    }
    let Some(parent) = ops
        .prompt
        .as_ref()
        .filter(|prompt| prompt.kind() == PromptKind::Paste)
        .map(|prompt| prompt.parent().to_path_buf())
    else {
        // The prompt was dismissed while the file was written.
        ops.paste_staging = None;
        return idle();
    };
    match result {
        Ok(file) => {
            ops.prompt = None;
            ops.pending_paste = None;
            (check_external_copy(vec![file], parent), Vec::new())
        }
        Err(reason) => {
            ops.paste_staging = None;
            ops.set_prompt_busy(false);
            ops.set_prompt_error_text(&reason);
            idle()
        }
    }
}
