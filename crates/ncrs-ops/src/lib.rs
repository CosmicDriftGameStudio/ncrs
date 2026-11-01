//! The file operations: the copy and move dialog with its progress mask, the
//! answer to a taken name, the delete confirmation and the trash view.

pub mod action;
pub mod conflict;
pub mod ctx;
pub mod delete;
pub mod delete_flow;
pub mod message;
pub mod restore_flow;
pub mod state;
pub mod transfer;
pub mod trash;
pub mod update;

pub use action::{Action, JobBuilder, StartJob, TransferAction, TrashAction};
pub use ctx::{Ctx, FlowCtx};
pub use delete_flow::{
    confirm_delete, delete_failure, delete_row_done, delete_subject, end_delete, open_delete,
    DeleteRequest, Outcome,
};
pub use message::{ConflictChoice, OpsMessage, TransferDialogMessage, TrashMessage};
pub use restore_flow::{
    end_restore, newest_entries_from, restore_last_deletion, restore_listed_deletion, restore_one,
    restore_row_done, resume_restore, start_restore,
};
pub use state::{Deletion, Ops, Restoration};
pub use transfer::{Policy, Refusal, Scan, TransferDialog};
pub use trash::TrashView;
pub use update::{update_transfer_dialog, update_trash};
