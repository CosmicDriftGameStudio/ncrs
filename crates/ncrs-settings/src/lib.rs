//! The settings: the settings page, the key bindings page, the fetch of color
//! themes and the pages of the text recognition and AI models.

pub mod ai_model;
pub mod key_bindings;
pub mod message;
pub mod ocr_models;
pub mod open_vsx;
pub mod settings;
pub mod theme_fetch;
pub mod vsix;

pub use message::{
    AiModelMessage, KeyBindingsMessage, OcrModelsMessage, SettingChange, SettingField,
    SettingsMessage, ThemeFetchMessage,
};
