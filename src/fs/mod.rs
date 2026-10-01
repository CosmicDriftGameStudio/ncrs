//! Filesystem layer. Knows nothing about the UI.

mod delete;
mod entry;
mod ops;
mod reader;
pub mod transfer;

pub use delete::{remove_permanently, SystemTrash, Trash};
pub use entry::FileEntry;
pub use ops::{create_dir, CreateDirError};
pub use reader::{home_dir, read_directory, start_dir, ReadError};
