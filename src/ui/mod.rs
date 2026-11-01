//! UI components. Every component is a pure function `state -> Element`;
//! components are generic over the message type where they emit messages,
//! so they can be reused in other contexts.

pub mod about;
pub use ncrs_files::context_menu_view as context_menu;
pub use ncrs_palette::command_line::view as command_line;
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
pub use ncrs_files::volumes_view as volumes;

pub use ncrs_ui_kit::{fkeys, format, frame, kit, theme};
pub use panel::PanelState;

#[cfg(test)]
pub use ncrs_hermes_ui::ui::filing_status;
pub use ncrs_hermes_ui::ui::{
    filing, filing_setup, hermes, hermes_flow, hermes_overview, hermes_setup, quick_fix,
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
