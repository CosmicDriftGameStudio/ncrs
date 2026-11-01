//! The state changes of the settings. What reaches outside them comes back as
//! actions.

mod keys;
mod models;
mod page;
mod theme;

use iced::Task;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::Settings;

pub use self::models::ocr_models_present;
pub use self::page::{close_settings, fresh_settings_state, open_settings, show_setting};

pub type Outcome = (Task<Message>, Vec<Action>);

pub fn update(settings: &mut Settings, message: Message, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = match message {
        Message::Settings(page_message) => page::update(settings, page_message, ctx, &mut actions),
        Message::KeyBindings(keys_message) => {
            keys::update(settings, keys_message, ctx, &mut actions)
        }
        Message::ThemeFetch(theme_message) => {
            theme::update(settings, theme_message, ctx, &mut actions)
        }
        Message::OcrModels(ocr_message) => {
            models::update_ocr(settings, ocr_message, ctx, &mut actions)
        }
        Message::AiModel(ai_message) => models::update_ai(settings, ai_message, ctx, &mut actions),
    };
    (task, actions)
}

/// The press of the button next to Cancel on the AI model card: it stops a
/// running download, otherwise it starts or continues one.
pub fn press_ai_model_button(settings: &mut Settings, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = models::press_ai_model_button(settings, ctx, &mut actions);
    (task, actions)
}
