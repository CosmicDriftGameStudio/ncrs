//! The command line below the panels, Quick Open and the messages of the
//! command list: their state, messages and views.

pub mod command_line;
mod focus;
pub mod message;
pub mod quick_open;

pub use message::{CommandLineMessage, CommandPaletteMessage, QuickOpenMessage};
