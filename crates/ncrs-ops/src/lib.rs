//! The file operations: the copy and move dialog with its progress mask, the
//! answer to a taken name, the delete confirmation and the trash view.

pub mod action;
pub mod conflict;
pub mod ctx;
pub mod delete;
pub mod message;
pub mod transfer;
pub mod trash;
pub mod update;

pub use action::{TransferAction, TrashAction};
pub use ctx::Ctx;
pub use message::{ConflictChoice, OpsMessage, TransferDialogMessage, TrashMessage};
pub use transfer::{Policy, Refusal, Scan, TransferDialog};
pub use trash::TrashView;
pub use update::{update_transfer_dialog, update_trash};
