//! The config file and what is read from it: the settings, the key bindings,
//! the colors of the theme and the list of commands. Knows nothing about the
//! windows that show or change them.

pub mod commands;
pub mod config;
pub mod file_list_tags;
pub mod keymap;
pub mod palette;
pub mod settings_page;
pub mod vscode_theme;

pub use settings_page::{SettingsFeature, SettingsTarget};
