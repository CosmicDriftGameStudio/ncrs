//! The settings page: its state and its view.

pub mod columns;
mod own_server;
mod state;
mod usage_state;
pub mod view;

pub use own_server::AiCheck;
pub use state::*;
pub use usage_state::{DEFAULT_USAGE_DAYS, USAGE_PERIODS};
