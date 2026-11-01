//! The settings: the settings page, the key bindings page, the fetch of color
//! themes and the pages of the text recognition and AI models.

pub mod action;
pub mod ai_model;
pub mod ctx;
pub mod key_bindings;
pub mod message;
pub mod ocr_models;
pub mod open_vsx;
pub mod settings;
pub mod state;
pub mod theme_fetch;
pub mod theme_file;
pub mod update;
pub mod vsix;
pub mod write;

pub use action::Action;
pub use ctx::{Ctx, Services};
pub use message::{
    AiModelMessage, KeyBindingsMessage, Message, OcrModelsMessage, SettingChange, SettingField,
    SettingsMessage, ThemeFetchMessage,
};
pub use state::Settings;
pub use update::{
    close_settings, fresh_settings_state, ocr_models_present, open_settings, press_ai_model_button,
    show_setting, update,
};
