//! Filesystem layer. Knows nothing about the UI.

mod delete;
mod entry;
mod open;
mod ops;
mod reader;
mod sort;
pub mod transfer;
mod volumes;

pub use delete::{remove_permanently, SystemTrash, Trash};
pub use entry::FileEntry;
pub use open::{could_execute, open_command, Launcher, OpenKind, SystemLauncher};
pub use ops::{create_dir, CreateDirError};
pub use reader::{home_dir, read_directory, start_dir, ReadError};
pub use sort::{sort_entries, SortColumn, SortKey};
pub use volumes::{list_volumes, Volume};
