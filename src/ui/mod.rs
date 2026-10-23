//! UI components. Every component is a pure function `state -> Element`;
//! components are generic over the message type where they emit messages,
//! so they can be reused in other contexts.

pub mod command_palette;
pub mod conflict;
pub mod context_menu;
pub mod delete;
pub mod dialog;
pub mod filing;
pub mod filing_setup;
pub mod filing_status;
pub mod fkeys;
pub mod format;
pub mod header;
pub mod key_intercept;
pub mod layout;
pub mod panel;
pub mod picker;
pub mod quick_open;
pub mod search;
pub mod statusbar;
pub mod theme;
pub mod volumes;

pub use panel::{PanelProps, PanelState};
