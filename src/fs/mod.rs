//! Filesystem layer. Knows nothing about the UI.

mod entry;
mod reader;

pub use entry::FileEntry;
pub use reader::{home_dir, read_directory, root_of};
