//! The Index and AI pages: the folder list and the models.

use std::path::PathBuf;

use iced::Task;
use ncrs_config::config::FilingLocation;
use ncrs_config::repos::ReposSettings;
use ncrs_config::space::SpaceSettings;
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
        SettingsMessage::SpaceListKey(key) => space_list_key(settings, key, ctx, actions),
        SettingsMessage::FolderPicked { path: None, .. } => Task::none(),
        SettingsMessage::FolderPicked {
            kind,
            path: Some(path),
        } if ctx.sandbox_active => {
            actions.push(Action::GrantFolder { kind, path });
            Task::none()
        }
        SettingsMessage::FolderPicked {
            kind: kind @ (AddAs::RepoSource | AddAs::RepoSkip),
            path: Some(path),
        }
        | SettingsMessage::FolderGranted {
            kind: kind @ (AddAs::RepoSource | AddAs::RepoSkip),
            path,
            result: Ok(()),
        } => change_repos(settings, ctx, |current| {
            repos_with_folder(current, kind, canonical(path))
        }),
        SettingsMessage::FolderPicked {
            kind: kind @ (AddAs::SpaceRegenerable | AddAs::SpaceNeverTouch),
            path: Some(path),
        }
        | SettingsMessage::FolderGranted {
            kind: kind @ (AddAs::SpaceRegenerable | AddAs::SpaceNeverTouch),
            path,
            result: Ok(()),
        } => change_space(settings, ctx, |current| {
            folders::space_with_folder(current, kind, canonical(path))
        }),
        SettingsMessage::ScanRepos => {
            actions.push(Action::ScanRepos);
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
            kind: AddAs::Indexed,
            path: Some(path),
        }
        | SettingsMessage::FolderGranted {
            kind: AddAs::Indexed,
            path,
            result: Ok(()),
        } => change_locations(settings, ctx, |list| folders::added(list, canonical(path))),
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
        SettingsMessage::AddOwnDetail(detail) => in_settings(settings, ctx, |state| {
            state.submit(crate::message::SettingField::Own(detail))
        }),
        SettingsMessage::FactsLoaded(loaded) => {
            let was_failing = settings
                .page
                .as_ref()
                .and_then(|state| state.loaded.as_ref())
                .is_some_and(|before| before.error.is_some());
            if let (Some(reason), false) = (&loaded.error, was_failing) {
                actions.push(Action::ShowNotice(fill_template(
                    ctx.lang.text(Msg::SettingsFactsReadFailed),
                    &[("{reason}", reason)],
                )));
            }
            page_flag(settings, |state| {
                state.loaded = Some(loaded);
                state.facts_in_flight = false;
            })
        }
        SettingsMessage::TogglePause => {
            actions.push(Action::TogglePause);
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
    ncrs_intake::canonical_path(&path).unwrap_or(path)
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

fn change_repos(
    settings: &mut Settings,
    ctx: &Ctx<'_>,
    edit: impl FnOnce(&ReposSettings) -> ReposSettings,
) -> Task<Message> {
    let Some(current) = settings
        .page
        .as_ref()
        .and_then(|state| state.config())
        .map(|config| config.repos.clone())
    else {
        return Task::none();
    };
    write_setting(settings, SettingChange::Repos(edit(&current)), ctx)
}

/// `path` as a search folder, or its name as a name the search skips; what is
/// there already stays as it is.
fn repos_with_folder(current: &ReposSettings, kind: AddAs, path: PathBuf) -> ReposSettings {
    let mut next = current.clone();
    match kind {
        AddAs::RepoSource if !next.sources.contains(&path) => next.sources.push(path),
        AddAs::RepoSkip => {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .filter(|name| !next.skip.contains(name));
            next.skip.extend(name);
        }
        AddAs::RepoSource
        | AddAs::Indexed
        | AddAs::Favourite
        | AddAs::SpaceRegenerable
        | AddAs::SpaceNeverTouch
        | AddAs::SpaceHome => {}
    }
    next
}

/// ⌘N and ⌫ on the Platz page: add to the folder list under the cursor, or remove the folder there.
fn space_list_key(
    settings: &mut Settings,
    key: crate::message::SpaceListKey,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    use crate::focus::Area;
    use crate::message::SpaceListKey;
    use crate::settings::view::space_list_target;
    let Some(state) = settings.page.as_ref() else {
        return Task::none();
    };
    if state.feature != ncrs_config::SettingsFeature::Space
        || state.focus.area != Area::Rows
        || state.focus.field
        || !state.query.is_empty()
    {
        return Task::none();
    }
    let Some(current) = state.config().map(|config| config.space.clone()) else {
        return Task::none();
    };
    let Some(target) = space_list_target(
        state.focus.row,
        &current,
        &state.space_drives,
        state.sandboxed,
    ) else {
        return Task::none();
    };
    match (key, target.folder) {
        (SpaceListKey::Add, _) => {
            actions.push(Action::PickFolder(target.kind));
            Task::none()
        }
        (SpaceListKey::Remove, Some(position)) => change_space(settings, ctx, |current| {
            let mut next = current.clone();
            match target.kind {
                AddAs::SpaceRegenerable => next.own_regenerable.remove(position),
                _ => next.never_touch.remove(position),
            };
            next
        }),
        (SpaceListKey::Remove, None) => Task::none(),
    }
}

fn change_space(
    settings: &mut Settings,
    ctx: &Ctx<'_>,
    edit: impl FnOnce(&SpaceSettings) -> SpaceSettings,
) -> Task<Message> {
    let Some(current) = settings
        .page
        .as_ref()
        .and_then(|state| state.config())
        .map(|config| config.space.clone())
    else {
        return Task::none();
    };
    write_setting(settings, SettingChange::Space(edit(&current)), ctx)
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
