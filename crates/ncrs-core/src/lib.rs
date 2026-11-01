//! Plain data types several features share. No logic beyond what a type needs
//! to be built, and no UI.

mod entry;
mod folder_size;
pub mod selection;

pub use entry::FileEntry;
pub use folder_size::FolderSize;
pub use selection::{Selection, SelectionSet};
