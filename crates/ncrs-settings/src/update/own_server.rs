//! The own server of the AI page (key, test, save, agreement) and the Usage page's log.

use iced::Task;
use ncrs_ai::Secret;
use ncrs_config::config::check_ai_endpoint;
use ncrs_i18n::{fill_template, Msg};

use super::page::in_settings;
use crate::ai_server::{AiCheckJob, AiCheckResult};
use crate::ctx::Ctx;
use crate::message::{Message, SettingField, SettingsMessage};
use crate::settings::{AiCheck, FieldProblem, SettingsState, USAGE_PERIODS};
use crate::state::Settings;

pub fn update(settings: &mut Settings, message: SettingsMessage, ctx: &Ctx<'_>) -> Task<Message> {
    match message {
        SettingsMessage::AiKey(key) => in_page(settings, |state| state.type_key(key)),
        SettingsMessage::ConfirmAiConsent => {
            in_settings(settings, ctx, SettingsState::agree_to_server)
        }
        SettingsMessage::TestAiServer => test_server(settings, ctx),
        SettingsMessage::AiChecked { endpoint, result } => {
            in_page(settings, |state| state.checked(&endpoint, result))
        }
        SettingsMessage::SaveServer => in_settings(settings, ctx, SettingsState::save_server),
        SettingsMessage::AiKeyStored(Ok(())) => Task::none(),
        SettingsMessage::AiKeyStored(Err(reason)) => {
            settings.failure = Some(fill_template(
                ctx.lang.text(Msg::SettingsAiKeyNotStored),
                &[("{reason}", &reason)],
            ));
            Task::none()
        }
        SettingsMessage::UsagePeriod(days) => in_page(settings, |state| state.usage_days = days),
        SettingsMessage::UsageLoaded(sums) => in_page(settings, |state| state.usage = Some(sums)),
        _ => Task::none(),
    }
}

fn in_page(settings: &mut Settings, edit: impl FnOnce(&mut SettingsState)) -> Task<Message> {
    if let Some(state) = settings.page.as_mut() {
        edit(state);
    }
    Task::none()
}

fn test_server(settings: &mut Settings, ctx: &Ctx<'_>) -> Task<Message> {
    let Some(state) = settings.page.as_mut() else {
        return Task::none();
    };
    let endpoint = state.typed_endpoint();
    if endpoint.is_empty() {
        state
            .problems
            .insert(SettingField::AiEndpoint, FieldProblem::Needed);
        return Task::none();
    }
    if let Err(problem) = check_ai_endpoint(&endpoint) {
        state
            .problems
            .insert(SettingField::AiEndpoint, FieldProblem::Endpoint(problem));
        return Task::none();
    }
    state.ai_check = AiCheck::Asking;
    let job = AiCheckJob {
        endpoint: endpoint.clone(),
        typed_key: state.ai_key.clone(),
        sandboxed: state.sandboxed,
    };
    let check = ctx.services.check_ai_server;
    Task::perform(
        tokio::task::spawn_blocking(move || check(&job)),
        move |joined| {
            let result = joined.unwrap_or(AiCheckResult::NoAnswer);
            Message::Settings(SettingsMessage::AiChecked { endpoint, result })
        },
    )
}

/// Keeps the key typed in the dialog once the address it belongs to is
/// written, so a key never sits in the keychain for an address that was
/// refused.
pub fn keep_key(endpoint: String, key: Secret, sandboxed: bool, ctx: &Ctx<'_>) -> Task<Message> {
    let store = ctx.services.store_ai_key;
    Task::perform(
        tokio::task::spawn_blocking(move || store(&endpoint, sandboxed, &key)),
        |joined| {
            let result = joined.unwrap_or_else(|join| Err(join.to_string()));
            Message::Settings(SettingsMessage::AiKeyStored(result))
        },
    )
}

/// Reads the usage log when the Usage page is opened.
pub fn load_usage(ctx: &Ctx<'_>) -> Task<Message> {
    let log = ctx.usage_log.clone();
    Task::perform(
        tokio::task::spawn_blocking(move || {
            ncrs_ai::usage_summaries(log.as_deref(), &USAGE_PERIODS)
        }),
        |joined| Message::Settings(SettingsMessage::UsageLoaded(joined.unwrap_or_default())),
    )
}
