//! The one entry of the file operations: every message that needs nothing but
//! the operations' state and the facts of [`FlowCtx`]. What also needs the
//! panels or the app's jobs (the copy dialog, confirming a delete or quit, the
//! trash, a rename, the abort) is still handled by the app.

use crate::archive_flow::archive_written;
use crate::batch_rename::flow::update_batch_rename;
use crate::checked_flow::{external_checked, pack_checked, paste_staged, transfer_checked};
use crate::ctx::FlowCtx;
use crate::delete_flow::{delete_row_done, idle, Outcome};
use crate::message::OpsMessage;
use crate::open_file::confirm_open_file;
use crate::permissions_flow::{permissions_done, update_permissions};
use crate::prompt_flow::{prompt_finished, submit_create_prompt};
use crate::state::Ops;
use crate::transfer_flow::{answer_conflict, transfer_row_done};
use crate::view_copy_flow::view_extracted;

/// Applies `message`; one the app handles itself changes nothing.
pub fn update(ops: &mut Ops, message: OpsMessage, ctx: &FlowCtx<'_>) -> Outcome {
    match message {
        OpsMessage::PromptSubmit => submit_create_prompt(ops, ctx),
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
        OpsMessage::Permissions(message) => update_permissions(ops, message),
        OpsMessage::BatchRename(message) => update_batch_rename(ops, message, ctx),
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
        OpsMessage::TransferDialog(_)
        | OpsMessage::DeleteConfirm
        | OpsMessage::QuitConfirm
        | OpsMessage::AbortJob
        | OpsMessage::Trash(_)
        | OpsMessage::Renamed { .. } => idle(),
    }
}

/// Whether [`update`] handles `message`, or the app does.
pub fn is_handled_by_update(message: &OpsMessage) -> bool {
    !matches!(
        message,
        OpsMessage::TransferDialog(_)
            | OpsMessage::DeleteConfirm
            | OpsMessage::QuitConfirm
            | OpsMessage::AbortJob
            | OpsMessage::Trash(_)
            | OpsMessage::Renamed { .. }
    )
}
