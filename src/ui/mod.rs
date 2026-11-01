//! UI components. Every component is a pure function `state -> Element`;
//! components are generic over the message type where they emit messages,
//! so they can be reused in other contexts.

pub mod about;
pub use ncrs_palette::command_line::view as command_line;
pub mod context_menu;
pub mod dialog;
pub mod grant;
pub mod layout;
pub mod panel;
pub use ncrs_files::panel_history;
pub use ncrs_ops::transfer::view as transfer;
pub use ncrs_ops::trash::view as trash;
pub use ncrs_ops::{conflict, delete};
pub use ncrs_palette::quick_open::view as quick_open;
pub use ncrs_remote::view as remote;
pub mod statusbar;
pub mod titlebar;
pub mod volumes;

pub use ncrs_ui_kit::{fkeys, format, frame, hermes_kit, kit, theme};
pub use panel::PanelState;

pub use ncrs_hermes_ui::ui::{
    filing, filing_setup, filing_status, hermes, hermes_flow, hermes_overview, hermes_setup,
    quick_fix, reorganisation_plan, review_list, rule_suggestions, rules, saved_queries, tags,
};
pub use ncrs_settings::ai_model::view as ai_model;
pub use ncrs_settings::key_bindings::view as key_bindings;
pub use ncrs_settings::ocr_models::view as ocr_models;
pub use ncrs_settings::theme_fetch::view as theme_fetch;

/// The settings page: its state and its view.
pub mod settings {
    pub use ncrs_settings::settings::view::*;
    pub use ncrs_settings::settings::*;
}
