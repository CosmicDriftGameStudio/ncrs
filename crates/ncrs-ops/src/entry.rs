//! The one entry of the file operations: every message is handled from the
//! operations' state and the facts of [`FlowCtx`]. What only the app can do
//! (the job queue, the panels, quitting) comes back as an [`Action`].

use crate::action::Action;
use crate::archive_flow::archive_written;
use crate::batch_rename::flow::update_batch_rename;
use crate::checked_flow::{external_checked, pack_checked, paste_staged, transfer_checked};
use crate::ctx::FlowCtx;
use crate::delete_flow::{actions, confirm_delete, delete_row_done, idle, Outcome};
use crate::message::OpsMessage;
use crate::open_file::confirm_open_file;
use crate::open_flow::{start_prompt_job, update_transfer_dialog_flow};
use crate::permissions_flow::{permissions_done, update_permissions};
use crate::prompt_flow::{prompt_finished, submit_create_prompt};
use crate::rename_flow::rename_done;
use crate::state::Ops;
use crate::transfer_flow::{answer_conflict, transfer_row_done};
use crate::trash_flow::update_trash_flow;
use crate::view_copy_flow::view_extracted;

/// Applies `message` and returns the task and the actions the app runs.
pub fn update(ops: &mut Ops, message: OpsMessage, ctx: &FlowCtx<'_>) -> Outcome {
    match message {
        OpsMessage::PromptSubmit if ops.valid_prompt_has_its_own_job() => {
            start_prompt_job(ops, ctx)
        }
        OpsMessage::PromptSubmit => submit_create_prompt(ops, ctx),
        OpsMessage::PromptInput(text) => {
            if let Some(prompt) = ops.prompt.as_mut() {
                prompt.set_name(text);
            }
            idle()
        }
        OpsMessage::PromptToggleKeepFolders => {
            ops.toggle_keep_folders();
            idle()
        }
        OpsMessage::PromptFormat(format) => {
            if let Some(prompt) = ops.prompt.as_mut() {
                prompt.set_archive_format(format);
            }
            idle()
        }
        OpsMessage::PromptCancel => {
            ops.prompt = None;
            ops.pending_paste = None;
            idle()
        }
        OpsMessage::PromptFinished {
            kind,
            request_id,
            result,
        } => prompt_finished(ops, kind, request_id, result, ctx),
        OpsMessage::TransferConflict(choice) => answer_conflict(ops, choice, ctx),
        OpsMessage::ToggleConflictAll => {
            ops.conflict_all = !ops.conflict_all;
            idle()
        }
        OpsMessage::DeleteCancel => {
            ops.delete_dialog = None;
            idle()
        }
        OpsMessage::OpenFileConfirm => confirm_open_file(ops),
        OpsMessage::OpenFileCancel => {
            ops.open_file_dialog = None;
            idle()
        }
        OpsMessage::QuitCancel => {
            ops.quit_dialog = false;
            idle()
        }
        OpsMessage::Permissions(permissions_message) => {
            update_permissions(ops, permissions_message)
        }
        OpsMessage::BatchRename(rename_message) => update_batch_rename(ops, rename_message, ctx),
        OpsMessage::DeleteRowDone {
            result,
            index,
            generation,
        } => delete_row_done(ops, result, index, generation, ctx),
        OpsMessage::TransferRowDone {
            result,
            skipped,
            index,
            generation,
        } => transfer_row_done(ops, result, skipped, index, generation, ctx),
        OpsMessage::ViewExtracted {
            generation,
            slot,
            result,
        } => view_extracted(ops, generation, slot, result, ctx),
        OpsMessage::ArchiveWritten { result, generation } => {
            archive_written(ops, generation, result, ctx)
        }
        OpsMessage::PermissionsDone { generation, result } => {
            permissions_done(ops, generation, result, ctx)
        }
        OpsMessage::TransferChecked {
            dialog,
            edit,
            kind,
            result,
        } => transfer_checked(ops, (dialog, edit, kind), result, ctx),
        OpsMessage::ExternalChecked { paths, result } => external_checked(ops, paths, result, ctx),
        OpsMessage::PackChecked { request_id, result } => {
            pack_checked(ops, request_id, result, ctx)
        }
        OpsMessage::PasteStaged { request_id, result } => paste_staged(ops, request_id, result),
        OpsMessage::TransferDialog(dialog_message) => {
            update_transfer_dialog_flow(ops, dialog_message, ctx)
        }
        OpsMessage::DeleteConfirm => confirm_delete(ops, ctx),
        OpsMessage::FilesHovered => {
            ops.drop_hover = true;
            idle()
        }
        OpsMessage::FilesHoverLeft => {
            ops.drop_hover = false;
            idle()
        }
        OpsMessage::FileDropped(path) => (
            ops.queue_dropped_file(path).map(OpsMessage::DropSettled),
            Vec::new(),
        ),
        OpsMessage::DropSettled(generation) => match ops.settled_drop(generation) {
            Some(paths) => actions(vec![Action::DropFiles(paths)]),
            None => idle(),
        },
        OpsMessage::QuitConfirm => actions(vec![Action::ConfirmQuit]),
        OpsMessage::AbortJob => actions(vec![Action::AbortJob]),
        OpsMessage::Trash(trash_message) => update_trash_flow(ops, trash_message, ctx),
        OpsMessage::Renamed {
            request_id,
            side,
            from,
            result,
        } => rename_done(ops, request_id, side, &from, result, ctx),
    }
}
