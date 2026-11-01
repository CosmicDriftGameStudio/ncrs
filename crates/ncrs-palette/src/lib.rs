//! The command line below the panels, Quick Open and the command list: their
//! state, messages and views.

pub mod command_line;
pub mod command_list;
pub mod message;
pub mod quick_open;

pub use message::{CommandLineMessage, CommandPaletteMessage, QuickOpenMessage};
