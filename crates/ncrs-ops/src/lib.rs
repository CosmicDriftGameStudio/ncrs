//! The file operations: the copy and move dialog with its progress mask, the
//! answer to a taken name, the delete confirmation and the trash view.

pub mod action;
pub mod archive_flow;
pub mod clipboard_flow;
pub mod clipboard_paste;
pub mod clipboard_text;
pub mod conflict;
pub mod ctx;
pub mod delete;
pub mod delete_flow;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod drag_out;
pub mod drop_paste;
pub mod message;
pub mod open_flow;
pub mod prompt;
pub mod prompt_flow;
pub mod prompt_view;
pub mod restore_flow;
pub mod row_failure;
pub mod state;
pub mod transfer;
pub mod transfer_flow;
pub mod transfer_open;
pub mod trash;
pub mod trash_flow;
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
pub use clipboard_flow::copy_files_to_clipboard;
pub use clipboard_text::{
    clipboard_text, clipboard_text_of_files, copied_notice, ClipboardKind, ClipboardText, PanelRows,
};
pub use ctx::{Ctx, FlowCtx};
pub use delete_flow::{
    confirm_delete, delete_failure, delete_row_done, delete_subject, end_delete, open_delete,
    DeleteRequest, Outcome,
};
pub use message::{ConflictChoice, OpsMessage, TransferDialogMessage, TrashMessage};
pub use open_flow::{
    confirm_transfer, copy_external_files, open_pack_prompt, open_transfer_dialog, paste_content,
    start_prompt_job, suggested_archive_name, update_transfer_dialog_flow, HitListFacts,
    PanelFacts, SelectionFacts,
};
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
pub use transfer_open::{conflicts_task, scan_task, unreachable_server_text, ValidTransfer};
pub use trash::TrashView;
pub use trash_flow::{menu_row as trash_menu_row, update_trash_flow};
pub use update::{update_transfer_dialog, update_trash};
pub use view_copy_flow::{end_extraction, start_view_copy, view_extracted};
