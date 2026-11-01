//! The state changes of the copy and move dialog and of the trash view. What
//! reaches outside them comes back as an action.

use ncrs_core::TransferKind;

use crate::action::{TransferAction, TrashAction};
use crate::ctx::Ctx;
use crate::message::{TransferDialogMessage, TrashMessage};
use crate::transfer::TransferDialog;
use crate::trash::TrashView;

/// Applies a message of the copy and move dialog to the open dialog, if any.
pub fn update_transfer_dialog(
    dialog: &mut Option<TransferDialog>,
    message: TransferDialogMessage,
) -> Option<TransferAction> {
    match message {
        TransferDialogMessage::Scanned { dialog: id, scan } => {
            if let Some(open) = dialog.as_mut().filter(|d| d.id() == id) {
                open.set_scan(scan);
            }
            None
        }
        TransferDialogMessage::ConflictsCounted {
            dialog: id,
            edit,
            count,
        } => {
            if let Some(open) = dialog.as_mut().filter(|d| d.id() == id) {
                open.set_conflicts(edit, count);
            }
            None
        }
        TransferDialogMessage::TargetInput(typed) => {
            let open = dialog.as_mut()?;
            open.type_target(typed);
            Some(TransferAction::CountConflicts)
        }
        TransferDialogMessage::SetKind(kind) => {
            if let Some(open) = dialog.as_mut() {
                open.set_kind(kind);
            }
            None
        }
        TransferDialogMessage::ToggleKind => {
            if let Some(open) = dialog.as_mut() {
                open.set_kind(match open.kind() {
                    TransferKind::Copy => TransferKind::Move,
                    TransferKind::Move => TransferKind::Copy,
                });
            }
            None
        }
        TransferDialogMessage::SetPolicy(policy) => {
            if let Some(open) = dialog.as_mut() {
                open.set_policy(policy);
            }
            None
        }
        TransferDialogMessage::ToggleList => {
            if let Some(open) = dialog.as_mut() {
                open.toggle_list();
            }
            None
        }
        TransferDialogMessage::Cancel => dialog.take().map(|_| TransferAction::Closed),
        TransferDialogMessage::Confirm => Some(TransferAction::Confirm),
        TransferDialogMessage::Background => Some(TransferAction::HideProgress),
        TransferDialogMessage::ShowProgress => Some(TransferAction::ShowProgress),
    }
}

/// Applies a message of the trash view: the listing and the cursor, the tags
/// and the scrolling change the view; the rest is the app's to do.
pub fn update_trash(
    trash: Option<&mut TrashView>,
    message: TrashMessage,
    ctx: &Ctx,
) -> Option<TrashAction> {
    let rows = ctx.trash_rows;
    match message {
        TrashMessage::Open => Some(TrashAction::Open),
        TrashMessage::Listed { generation, result } => {
            if generation == ctx.trash_generation {
                if let Some(listed) = trash {
                    listed.set_items(result, rows);
                }
            }
            None
        }
        TrashMessage::Close => Some(TrashAction::Close),
        TrashMessage::Restore => Some(TrashAction::Restore(
            trash.map(|shown| shown.action_items()).unwrap_or_default(),
        )),
        TrashMessage::RestoreRowDone {
            result,
            index,
            generation,
        } => Some(TrashAction::RestoreRowDone {
            result,
            index,
            generation,
        }),
        TrashMessage::RestoreLast => Some(TrashAction::RestoreLast),
        TrashMessage::LastListed { generation, result } => {
            Some(TrashAction::LastListed { generation, result })
        }
        TrashMessage::OpenInFinder => Some(TrashAction::OpenInFinder),
        view_message => {
            let view = trash?;
            match view_message {
                TrashMessage::Move(delta) => view.move_cursor(delta, rows),
                TrashMessage::Page(direction) => view.page(direction, rows),
                TrashMessage::First => view.first(rows),
                TrashMessage::Last => view.last(rows),
                TrashMessage::ToggleTag => view.toggle_tag(),
                TrashMessage::TagMove(delta) => view.tag_and_move(delta, rows),
                TrashMessage::TagAll => view.tag_all(),
                TrashMessage::ClearTags => view.clear_tags(),
                TrashMessage::RowClicked(index) => view.select(index, rows),
                TrashMessage::Scrolled(delta) => view.scroll(delta, rows),
                TrashMessage::Open
                | TrashMessage::Listed { .. }
                | TrashMessage::Close
                | TrashMessage::Restore
                | TrashMessage::RestoreRowDone { .. }
                | TrashMessage::RestoreLast
                | TrashMessage::LastListed { .. }
                | TrashMessage::OpenInFinder => {}
            }
            None
        }
    }
}
