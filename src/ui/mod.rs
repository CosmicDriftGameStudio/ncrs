//! UI components. Every component is a pure function `state -> Element`;
//! components are generic over the message type where they emit messages,
//! so they can be reused in other contexts.

pub mod about;
pub mod ai_model;
pub use ncrs_palette::command_line::view as command_line;
pub mod command_palette;
pub mod condition_editor;
pub mod context_menu;
pub mod dialog;
pub mod document_preview;
pub mod filing;
pub mod filing_pieces;
pub mod filing_setup;
pub mod filing_status;
pub mod grant;
pub mod hermes;
pub mod hermes_flow;
pub mod hermes_overview;
pub mod hermes_setup;
pub mod key_bindings;
pub mod layout;
pub mod ocr_models;
pub mod panel;
pub use ncrs_files::panel_history;
pub use ncrs_ops::transfer::view as transfer;
pub use ncrs_ops::trash::view as trash;
pub use ncrs_ops::{conflict, delete};
pub use ncrs_ui_kit::picker;
pub mod quick_fix;
pub use ncrs_palette::quick_open::view as quick_open;
pub mod remote;
pub mod reorganisation_plan;
pub mod review_list;
pub mod rule_suggestions;
pub mod rules;
pub mod saved_queries;
pub mod settings;
pub mod statusbar;
pub mod tags;
pub mod theme_fetch;
pub mod titlebar;
pub mod volumes;

pub use ncrs_ui_kit::{fkeys, format, frame, hermes_kit, kit, theme};
pub use panel::PanelState;
