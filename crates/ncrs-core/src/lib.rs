//! Plain data types several features share. No logic beyond what a type needs
//! to be built, and no UI.

mod entry;
mod folder_size;
mod index_not_open;
mod panel_side;
pub mod selection;

pub use entry::FileEntry;
pub use folder_size::FolderSize;
pub use index_not_open::INDEX_NOT_OPEN;
pub use panel_side::PanelSide;
pub use selection::{Selection, SelectionSet};
