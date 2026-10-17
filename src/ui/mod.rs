//! UI components. Every component is a pure function `state -> Element`;
//! components are generic over the message type where they emit messages,
//! so they can be reused in other contexts.

pub mod conflict;
pub mod context_menu;
pub mod delete;
pub mod dialog;
pub mod fkeys;
pub mod format;
pub mod header;
pub mod key_intercept;
pub mod layout;
pub mod panel;
pub mod quick_open;
pub mod search;
pub mod statusbar;
pub mod theme;
pub mod volumes;

pub use panel::{PanelProps, PanelState};
