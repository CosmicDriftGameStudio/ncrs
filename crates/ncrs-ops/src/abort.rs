//! Stopping whichever file operation is running.

use crate::delete_flow::{end_delete, idle, Outcome};
use crate::restore_flow::end_restore;
use crate::state::Ops;
use crate::transfer_flow::end_transfer;
use crate::{abort_archive_write, abort_batch_rename, abort_permissions, end_extraction, FlowCtx};

/// Ends the flow whose job has just been aborted. Which flow that is lives
/// here, so a flow added to the crate is stopped by the same call.
pub fn abort_running(ops: &mut Ops, ctx: &FlowCtx<'_>) -> Outcome {
    if ops.extracting.is_some() {
        end_extraction(ops);
        idle()
    } else if let Some(outcome) = abort_archive_write(ops) {
        outcome
    } else if let Some(outcome) = abort_batch_rename(ops) {
        outcome
    } else if let Some(outcome) = abort_permissions(ops) {
        outcome
    } else if ops.deleting.is_some() {
        end_delete(ops, ctx)
    } else if ops.restoring.is_some() {
        end_restore(ops, ctx, false)
    } else {
        end_transfer(ops, ctx.lang)
    }
}
