//! UI components. Every component is a pure function `state -> Element`;
//! components are generic over the message type where they emit messages,
//! so they can be reused in other contexts.

pub mod conflict;
pub mod delete;
pub mod dialog;
pub mod fkeys;
pub mod format;
pub mod header;
pub mod layout;
pub mod panel;
pub mod statusbar;
pub mod theme;

pub use panel::{PanelProps, PanelState};
