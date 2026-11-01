//! UI components. Every component is a pure function `state -> Element`;
//! components are generic over the message type where they emit messages,
//! so they can be reused in other contexts.

pub mod about;
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
pub mod layout;
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
pub mod statusbar;
pub mod tags;
pub mod titlebar;
pub mod volumes;

pub use ncrs_ui_kit::{fkeys, format, frame, hermes_kit, kit, theme};
pub use panel::PanelState;

pub use ncrs_settings::ai_model::view as ai_model;
pub use ncrs_settings::key_bindings::view as key_bindings;
pub use ncrs_settings::ocr_models::view as ocr_models;
pub use ncrs_settings::theme_fetch::view as theme_fetch;

/// The settings page: its state and its view.
pub mod settings {
    pub use ncrs_settings::settings::view::*;
    pub use ncrs_settings::settings::*;
}
