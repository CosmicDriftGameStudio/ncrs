//! The file operations: the copy and move dialog with its progress mask, the
//! answer to a taken name, the delete confirmation and the trash view.

pub mod action;
pub mod archive_flow;
pub mod conflict;
pub mod ctx;
pub mod delete;
pub mod delete_flow;
pub mod message;
pub mod restore_flow;
pub mod row_failure;
pub mod state;
pub mod transfer;
pub mod transfer_flow;
pub mod trash;
pub mod update;
pub mod view_copy_flow;

pub use action::{
    Action, JobBuilder, ProgressJobBuilder, StartJob, StartProgressJob, TransferAction,
    TrashAction, ViewOrigin,
};
pub use archive_flow::{
    abort_archive_write, archive_written, begin_copy_into_archive, start_archive_delete,
    start_pack, PackRequest,
};
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
pub use row_failure::RowFailure;
pub use state::{
    ArchiveWrite, ArchiveWriteKind, Deletion, Extracting, Ops, PendingConflict, Restoration,
    Transfer,
};
pub use transfer::{Policy, Refusal, Scan, TransferDialog};
pub use transfer_flow::{
    answer_conflict, begin_transfer, child_path, end_transfer, refusal_text,
    start_validated_transfer, transfer_row_done,
};
pub use trash::TrashView;
pub use update::{update_transfer_dialog, update_trash};
pub use view_copy_flow::{end_extraction, start_view_copy, view_extracted};
