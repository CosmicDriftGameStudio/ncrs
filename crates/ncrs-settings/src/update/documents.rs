//! The Index and Hermes pages: the folder lists, the models and the ways on
//! to the chat and the status pages.

use std::path::PathBuf;

use iced::Task;
use ncrs_config::config::FilingLocation;
use ncrs_i18n::{fill_template, Msg};

use super::models;
use super::page::in_settings;
use crate::action::Action;
use crate::ctx::Ctx;
use crate::facts::ModelKind;
use crate::folders::{self, AddAs};
use crate::message::{AiModelMessage, Message, SettingChange, SettingsMessage};
use crate::model_removal::remove_model_files;
use crate::state::Settings;
use crate::write::write_setting;

pub fn update(
    settings: &mut Settings,
    message: SettingsMessage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match message {
        SettingsMessage::AddFolder(kind) => {
            actions.push(Action::PickFolder(kind));
            Task::none()
        }
        SettingsMessage::FolderPicked { path: None, .. } => Task::none(),
        SettingsMessage::FolderPicked {
            kind,
            path: Some(path),
        } if ctx.sandbox_active => {
            actions.push(Action::GrantFolder { kind, path });
            Task::none()
        }
        SettingsMessage::FolderPicked {
            kind: AddAs::Favourite,
            path: Some(path),
        }
        | SettingsMessage::FolderGranted {
            kind: AddAs::Favourite,
            path,
            result: Ok(()),
        } => {
            actions.push(Action::AddFavourite(canonical(path)));
            Task::none()
        }
        SettingsMessage::FolderPicked {
            kind,
            path: Some(path),
        }
        | SettingsMessage::FolderGranted {
            kind,
            path,
            result: Ok(()),
        } => change_locations(settings, ctx, |list| {
            folders::added(list, canonical(path), kind)
        }),
        SettingsMessage::FolderGranted {
            path,
            result: Err(reason),
            ..
        } => {
            settings.failure = Some(fill_template(
                ctx.lang.text(Msg::GrantFailed),
                &[("{folder}", &path.to_string_lossy()), ("{reason}", &reason)],
            ));
            Task::none()
        }
        SettingsMessage::RemoveFolder(path) => {
            change_locations(settings, ctx, |list| folders::removed(list, &path))
        }
        SettingsMessage::EditFolder(path) => {
            page_flag(settings, |state| state.folder_dialog = path)
        }
        SettingsMessage::ReplaceFolder(edited) => {
            change_locations(settings, ctx, |list| folders::replaced(list, edited))
        }
        SettingsMessage::AddOwnDetail(detail) => in_settings(settings, ctx, |state| {
            state.submit(crate::message::SettingField::Own(detail))
        }),
        SettingsMessage::RunUndone(Ok(())) => {
            page_flag(settings, |state| state.facts_requested = false)
        }
        SettingsMessage::RunUndone(Err(reason)) => {
            actions.push(Action::ShowNotice(fill_template(
                ctx.lang.text(Msg::SettingsHermesHistoryUndoFailed),
                &[("{reason}", &reason)],
            )));
            Task::none()
        }
        SettingsMessage::FactsLoaded(loaded) => page_flag(settings, |state| {
            state.loaded = Some(loaded);
            state.facts_in_flight = false;
        }),
        SettingsMessage::ToggleAllAreas => {
            page_flag(settings, |state| state.all_areas = !state.all_areas)
        }
        SettingsMessage::ToggleAllRuns => {
            page_flag(settings, |state| state.all_runs = !state.all_runs)
        }
        SettingsMessage::ServerDialog(open) => page_flag(settings, |state| {
            state.server_dialog = open;
            if !open {
                // Cancelled: the typed key and the test belong to this
                // opening of the dialog only.
                state.ai_key = None;
                state.ai_check = crate::settings::AiCheck::Idle;
            }
        }),
        SettingsMessage::OpenChat(area) => {
            actions.push(Action::OpenChat { area });
            Task::none()
        }
        SettingsMessage::TogglePause => {
            actions.push(Action::TogglePause);
            Task::none()
        }
        SettingsMessage::UndoRun(batch) => {
            actions.push(Action::UndoRun(batch));
            Task::none()
        }
        SettingsMessage::OpenAiModel => {
            models::update_ai(settings, AiModelMessage::Open, ctx, actions)
        }
        SettingsMessage::AskRemoveModel(kind) => {
            page_flag(settings, |state| state.model_to_remove = Some(kind))
        }
        SettingsMessage::CancelRemoveModel => {
            page_flag(settings, |state| state.model_to_remove = None)
        }
        SettingsMessage::ConfirmRemoveModel => confirm_removal(settings, ctx),
        SettingsMessage::ModelRemoved { kind, result } => {
            model_removed(settings, kind, result, ctx, actions)
        }
        _ => Task::none(),
    }
}

/// The folder as the rules compare it; one that cannot be resolved is kept as
/// the dialog gave it.
fn canonical(path: PathBuf) -> PathBuf {
    std::fs::canonicalize(&path).unwrap_or(path)
}

fn page_flag(
    settings: &mut Settings,
    set: impl FnOnce(&mut crate::settings::SettingsState),
) -> Task<Message> {
    if let Some(state) = settings.page.as_mut() {
        set(state);
    }
    Task::none()
}

/// Writes the list `edit` makes of the list in the file. Nothing when the
/// file does not load: then nothing can be written.
fn change_locations(
    settings: &mut Settings,
    ctx: &Ctx<'_>,
    edit: impl FnOnce(&[FilingLocation]) -> Vec<FilingLocation>,
) -> Task<Message> {
    let Some(current) = settings
        .page
        .as_ref()
        .and_then(|state| state.config())
        .map(|config| config.filing.locations.clone())
    else {
        return Task::none();
    };
    write_setting(settings, SettingChange::Locations(edit(&current)), ctx)
}

fn confirm_removal(settings: &mut Settings, ctx: &Ctx<'_>) -> Task<Message> {
    let Some(kind) = settings
        .page
        .as_mut()
        .and_then(|state| state.model_to_remove.take())
    else {
        return Task::none();
    };
    let configured_dir = match kind {
        ModelKind::Ocr => ctx.ocr_models_dir.clone(),
        ModelKind::Ai => ctx.ai_models_dir.clone(),
    };
    let Some(dir) = configured_dir else {
        return Task::none();
    };
    Task::perform(
        async move { tokio::task::spawn_blocking(move || remove_model_files(kind, &dir)).await },
        move |joined| {
            Message::Settings(SettingsMessage::ModelRemoved {
                kind,
                result: joined.unwrap_or_else(|join| Err(join.to_string())),
            })
        },
    )
}

fn model_removed(
    settings: &mut Settings,
    kind: ModelKind,
    result: Result<(), String>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match result {
        Ok(()) => match kind {
            ModelKind::Ocr => {
                if let Some(state) = settings.page.as_mut() {
                    state.ocr_models_present = false;
                }
            }
            ModelKind::Ai => {
                if let Some(state) = settings.page.as_mut() {
                    state.ai_model_present = false;
                }
                actions.push(Action::RefreshAiModel);
            }
        },
        Err(reason) => actions.push(Action::ShowNotice(fill_template(
            ctx.lang.text(Msg::SettingsModelRemoveFailed),
            &[("{reason}", &reason)],
        ))),
    }
    Task::none()
}
