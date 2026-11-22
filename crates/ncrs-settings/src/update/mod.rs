//! The state changes of the settings. What reaches outside them comes back as
//! actions.

#[cfg(test)]
mod consent_tests;
mod documents;
mod keys;
mod models;
mod notes;
mod own_server;
mod page;
mod theme;

use iced::Task;
use ncrs_config::SettingsFeature;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::Settings;

pub use self::models::ocr_models_present;
pub use self::page::{close_settings, fresh_settings_state, open_settings, show_setting};

pub type Outcome = (Task<Message>, Vec<Action>);

/// What decides which entry of the sidebar is chosen and which are listed.
fn shown_page(settings: &Settings) -> Option<(SettingsFeature, &str)> {
    settings
        .page
        .as_ref()
        .map(|state| (state.feature, state.query.as_str()))
}

/// The task of `task` plus, when the page or the search changed, the scroll
/// that keeps the chosen sidebar entry in view.
fn keeping_chosen_in_view(
    settings: &Settings,
    before: Option<(SettingsFeature, String)>,
    task: Task<Message>,
) -> Task<Message> {
    let after = shown_page(settings).map(|(feature, query)| (feature, query.to_owned()));
    if after.is_some() && after != before {
        Task::batch([task, ncrs_ui_kit::nav_list::chosen_into_view()])
    } else {
        task
    }
}

pub fn update(settings: &mut Settings, message: Message, ctx: &Ctx<'_>) -> Outcome {
    let before = shown_page(settings).map(|(feature, query)| (feature, query.to_owned()));
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
    (keeping_chosen_in_view(settings, before, task), actions)
}

/// The press of the button next to Cancel on the AI model card: it stops a
/// running download, otherwise it starts or continues one.
pub fn press_ai_model_button(settings: &mut Settings, ctx: &Ctx<'_>) -> Outcome {
    let mut actions = Vec::new();
    let task = models::press_ai_model_button(settings, ctx, &mut actions);
    (task, actions)
}
