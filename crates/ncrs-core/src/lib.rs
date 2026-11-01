//! Plain data types several features share. No logic beyond what a type needs
//! to be built, and no UI.

mod entry;
mod folder_size;
mod found_document;
mod function_keys;
mod index_not_open;
mod panel_side;
pub mod selection;
mod sort_key;
mod transfer_kind;

pub use entry::FileEntry;
pub use folder_size::FolderSize;
pub use found_document::{FoundDocument, TextSource};
pub use function_keys::FUNCTION_KEY_COUNT;
pub use index_not_open::INDEX_NOT_OPEN;
pub use panel_side::PanelSide;
pub use selection::{Selection, SelectionSet};
pub use sort_key::{SortColumn, SortKey};
pub use transfer_kind::TransferKind;
