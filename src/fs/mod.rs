//! Filesystem layer. Knows nothing about the UI.

mod entry;
mod ops;
mod reader;
mod transfer;

pub use entry::FileEntry;
pub use ops::{create_dir, CreateDirError};
pub use reader::{home_dir, read_directory, start_dir, ReadError};
