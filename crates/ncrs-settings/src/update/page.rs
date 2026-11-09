use iced::Task;
use ncrs_config::config::{self, AiProviderChoice, Config, FilingWriteError};
use ncrs_config::SettingsFeature;
use ncrs_fs::runs_in_app_sandbox;
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::focus::release_focus;
use ncrs_ui_kit::font;

use super::{documents, keys, models, own_server, theme};
use crate::action::Action;
use crate::ai_model::InstallOutcome;
use crate::ctx::Ctx;
use crate::message::{
    ColumnsEdit, KeyBindingsMessage, Message, OcrModelsMessage, SettingChange, SettingsMessage,
    ThemeFetchMessage,
};
use crate::settings::{columns, SettingsState};
use crate::state::Settings;
use crate::theme_file::{
    copy_picked_theme_file, theme_file_entry, IncludeNotCopied, IncludeProblem,
};
use crate::write::{change_failure, write_failure, write_setting};

pub fn update(
    settings: &mut Settings,
    message: SettingsMessage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match message {
        SettingsMessage::Toggle if settings.page.is_some() => {
            leave_page(settings, ctx, close_settings)
        }
        SettingsMessage::Toggle => open_settings(settings, ctx),
        SettingsMessage::ToggleFileListTags => {
            let switched = ncrs_config::file_list_tags::FileListTags {
                show: !ctx.file_list_tags.show,
                ..ctx.file_list_tags.clone()
            };
            write_setting(settings, SettingChange::FileListTags(switched), ctx)
        }
        SettingsMessage::ChooseFileListTags => {
            show_setting(settings, ctx, SettingsFeature::Tags, None)
        }
        SettingsMessage::Close
            if settings
                .page
                .as_ref()
                .is_some_and(|state| state.dialog_is_open()) =>
        {
            if let Some(state) = settings.page.as_mut() {
                state.close_dialog();
            }
            Task::none()
        }
        SettingsMessage::Close => leave_page(settings, ctx, close_settings),
        SettingsMessage::Select(feature)
            if settings
                .page
                .as_ref()
                .is_some_and(|state| state.feature != feature) =>
        {
            let reading = page_data(feature, ctx);
            let left = leave_page(settings, ctx, |held| {
                if let Some(state) = held.page.as_mut() {
                    state.feature = feature;
                    state.marked_row = None;
                    state.focus = crate::focus::Focus::default();
                }
                Task::none()
            });
            Task::batch([left, reading])
        }
        SettingsMessage::Select(_) => in_settings(settings, ctx, |state| {
            state.marked_row = None;
            None
        }),
        SettingsMessage::SetFocus(focus) => in_settings(settings, ctx, |state| {
            state.focus = focus;
            None
        }),
        SettingsMessage::FocusThen(row, then) => {
            let focused = in_settings(settings, ctx, |state| {
                let same_row =
                    state.focus.area == crate::focus::Area::Rows && state.focus.row == row;
                if !same_row {
                    state.focus = crate::focus::Focus::in_rows(row);
                }
                None
            });
            match then {
                Some(inner) => Task::batch([focused, update(settings, *inner, ctx, actions)]),
                None => focused,
            }
        }
        // Needs the rows of the page; the app turns it into a step.
        SettingsMessage::FocusKey(_) => Task::none(),
        SettingsMessage::SearchChanged(query) => in_settings(settings, ctx, |state| {
            state.query = query;
            state.marked_row = None;
            None
        }),
        SettingsMessage::Edit { field, text } => in_settings(settings, ctx, |state| {
            state.edit(field, text);
            None
        }),
        SettingsMessage::Submit(field) => in_settings(settings, ctx, |state| state.submit(field)),
        SettingsMessage::ChooseProvider(provider) => {
            in_settings(settings, ctx, |state| state.choose_provider(provider))
        }
        SettingsMessage::ChooseTerminal(choice) => {
            in_settings(settings, ctx, |state| state.choose_terminal(choice))
        }
        SettingsMessage::PickProgram(kind) => {
            let apps_only = settings.page.as_ref().is_some_and(|state| state.sandboxed);
            actions.push(Action::PickProgram { kind, apps_only });
            Task::none()
        }
        SettingsMessage::ProgramPicked {
            kind,
            path: Some(path),
        } => in_settings(settings, ctx, |state| state.picked_program(kind, &path)),
        SettingsMessage::PickThemeFile => {
            actions.push(Action::PickThemeFile);
            Task::none()
        }
        SettingsMessage::ThemeFilePicked(Some(path)) => {
            theme_file_picked(settings, &path, ctx, actions)
        }
        SettingsMessage::ProgramPicked { path: None, .. }
        | SettingsMessage::ThemeFilePicked(None) => Task::none(),
        SettingsMessage::OpenThemeFetch => {
            theme::update(settings, ThemeFetchMessage::Open, ctx, actions)
        }
        SettingsMessage::OpenKeyBindings => {
            keys::update(settings, KeyBindingsMessage::Open, ctx, actions)
        }
        SettingsMessage::OpenOcrModels => {
            models::update_ocr(settings, OcrModelsMessage::Open, ctx, actions)
        }
        SettingsMessage::OpenServerAdd => {
            actions.push(Action::OpenServerAdd);
            Task::none()
        }
        SettingsMessage::OpenServer(url) => {
            actions.push(Action::OpenServer(url));
            Task::none()
        }
        SettingsMessage::Protection(position) => {
            actions.push(Action::Protection(position));
            Task::none()
        }
        forwarded @ (SettingsMessage::AddFolder(_)
        | SettingsMessage::FolderPicked { .. }
        | SettingsMessage::FolderGranted { .. }
        | SettingsMessage::RemoveFolder(_)
        | SettingsMessage::EditFolder(_)
        | SettingsMessage::ReplaceFolder(_)
        | SettingsMessage::AddOwnDetail(_)
        | SettingsMessage::ToggleAllAreas
        | SettingsMessage::ToggleAllRuns
        | SettingsMessage::ServerDialog(_)
        | SettingsMessage::OpenChat(_)
        | SettingsMessage::TogglePause
        | SettingsMessage::UndoRun(_)
        | SettingsMessage::FactsLoaded(_)
        | SettingsMessage::RunUndone(_)
        | SettingsMessage::OpenAiModel
        | SettingsMessage::AskRemoveModel(_)
        | SettingsMessage::CancelRemoveModel
        | SettingsMessage::ConfirmRemoveModel
        | SettingsMessage::ModelRemoved { .. }) => {
            documents::update(settings, forwarded, ctx, actions)
        }
        own @ (SettingsMessage::AiKey(_)
        | SettingsMessage::TestAiServer
        | SettingsMessage::AiChecked { .. }
        | SettingsMessage::UnderstandLeaving(_)
        | SettingsMessage::SaveServer
        | SettingsMessage::AiKeyStored(_)
        | SettingsMessage::UsagePeriod(_)
        | SettingsMessage::UsageLoaded(_)) => own_server::update(settings, own, ctx),
        SettingsMessage::Change(change) => write_setting(settings, change, ctx),
        SettingsMessage::Columns(edit) => columns_edit(settings, edit, ctx),
        SettingsMessage::Favourites(edit) => {
            actions.push(Action::EditFavourites(edit));
            Task::none()
        }
        SettingsMessage::Written { change, result } => {
            written(settings, change, result, ctx, actions)
        }
    }
}

fn columns_edit(settings: &mut Settings, edit: ColumnsEdit, ctx: &Ctx<'_>) -> Task<Message> {
    let Some(state) = settings.page.as_mut() else {
        return Task::none();
    };
    if let ColumnsEdit::Side(side) = edit {
        state.columns_side = side;
        return Task::none();
    }
    let current = state.panel_columns();
    let changed = columns::edited(&current, state.edited_columns_side(), edit);
    if changed == current {
        return Task::none();
    }
    write_setting(settings, SettingChange::Columns(changed), ctx)
}

/// Writes the values typed on the page without Enter, one after the
/// other, and runs `then`. A wrong value keeps the page as it is: it shows
/// at its row and `then` does not run.
fn leave_page(
    settings: &mut Settings,
    ctx: &Ctx<'_>,
    then: impl FnOnce(&mut Settings) -> Task<Message>,
) -> Task<Message> {
    let Some(changes) = settings.page.as_mut().and_then(SettingsState::leave) else {
        return Task::none();
    };
    let writes = changes
        .into_iter()
        .map(|change| write_setting(settings, change, ctx))
        .reduce(Task::chain)
        .unwrap_or_else(Task::none);
    Task::batch([then(settings), writes])
}

/// Runs `edit` on the open tab and writes the change it asks for.
pub(in crate::update) fn in_settings(
    settings: &mut Settings,
    ctx: &Ctx<'_>,
    edit: impl FnOnce(&mut SettingsState) -> Option<SettingChange>,
) -> Task<Message> {
    match settings.page.as_mut().and_then(edit) {
        Some(change) => write_setting(settings, change, ctx),
        None => Task::none(),
    }
}

fn theme_file_picked(
    settings: &mut Settings,
    path: &std::path::Path,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    if let (true, Some(config_path)) = (ctx.sandbox_active, ctx.config_path) {
        return match copy_picked_theme_file(config_path, path) {
            Ok(copy) => {
                let written =
                    write_setting(settings, SettingChange::VscodeTheme(Some(copy.entry)), ctx);
                if let Some(problem) = copy.include_not_copied {
                    actions.push(Action::ShowNotice(include_not_copied_text(&problem, ctx)));
                }
                written
            }
            Err(error) => {
                settings.failure = Some(write_failure(&error, ctx));
                Task::none()
            }
        };
    }
    match theme_file_entry(ctx.config_path, path) {
        Some(file) => write_setting(settings, SettingChange::VscodeTheme(Some(file)), ctx),
        None => {
            settings.failure = Some(ctx.lang.text(Msg::SettingsProblemPathNotText).to_owned());
            Task::none()
        }
    }
}

fn include_not_copied_text(problem: &IncludeNotCopied, ctx: &Ctx<'_>) -> String {
    let reason = match &problem.reason {
        IncludeProblem::OutsideFolder => ctx
            .lang
            .text(Msg::SettingsThemeIncludeOutsideFolder)
            .to_owned(),
        IncludeProblem::Unreadable(reason) => reason.clone(),
    };
    fill_template(
        ctx.lang.text(Msg::SettingsThemeIncludeNotCopied),
        &[("{include}", &problem.name), ("{reason}", &reason)],
    )
}

fn written(
    settings: &mut Settings,
    change: SettingChange,
    result: Result<Box<Config>, FilingWriteError>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match result {
        Ok(config) => {
            settings.failure = None;
            let mut keeping = Task::none();
            if let Some(state) = settings.page.as_mut() {
                state.written(&change, &config);
                if let SettingChange::AiProvider(AiProviderChoice::Http { endpoint, .. }) = &change
                {
                    if let Some(key) = state.ai_key.take() {
                        keeping = own_server::keep_key(endpoint.clone(), key, state.sandboxed, ctx);
                    }
                }
            }
            actions.push(Action::Apply { change, config });
            return keeping;
        }
        Err(error) => {
            if matches!(change, SettingChange::AiEnabled(true)) {
                if let Some(card) = settings.ai_model.as_mut() {
                    card.set_outcome(InstallOutcome::StillOff);
                }
            }
            if matches!(change, SettingChange::HermesEnabled(true)) {
                actions.push(Action::HermesNotTurnedOn(error.to_string()));
                if settings.page.is_none() {
                    return Task::none();
                }
            }
            let failure = change_failure(&error, &change, ctx);
            let at_its_row = match settings.page.as_mut() {
                Some(state) => state.refused(&change, failure.clone()),
                None => false,
            };
            if !at_its_row {
                if settings.page.is_some() {
                    settings.failure = Some(failure);
                } else {
                    actions.push(Action::ShowNotice(failure));
                }
            }
        }
    }
    Task::none()
}

pub fn open_settings(settings: &mut Settings, ctx: &Ctx<'_>) -> Task<Message> {
    if ctx.dialog_open {
        return Task::none();
    }
    settings.failure = None;
    settings.page = Some(fresh_settings_state(ctx));
    release_focus()
}

pub fn fresh_settings_state(ctx: &Ctx<'_>) -> SettingsState {
    let ocr_models_present = models::ocr_models_present(ctx.ocr_models_dir.as_deref());
    // Read again rather than kept from the start: the tab shows the values
    // as the file spells them, and the file may have changed since.
    let file = match ctx.config_path {
        Some(path) => config::load(path).map(Box::new),
        None => Ok(Box::default()),
    };
    let mut state = SettingsState::new(ocr_models_present, file, runs_in_app_sandbox());
    state.terminals = ncrs_fs::detect_terminals();
    state.installed_fonts = font::installed_monospace();
    state.system_font = font::system_font();
    state.ai_model_present = ctx
        .ai_models_dir
        .as_deref()
        .is_some_and(crate::ai_model::model_is_installed);
    state
}

pub fn close_settings(settings: &mut Settings) -> Task<Message> {
    if settings.page.take().is_some() {
        settings.failure = None;
        release_focus()
    } else {
        Task::none()
    }
}

/// Opens the settings tab, unless something blocks it, on `feature`, with
/// `row` in view.
pub fn show_setting(
    settings: &mut Settings,
    ctx: &Ctx<'_>,
    feature: SettingsFeature,
    row: Option<String>,
) -> Task<Message> {
    let reading = page_data(feature, ctx);
    if settings.page.is_some() {
        let left = leave_page(settings, ctx, |held| {
            if let Some(state) = held.page.as_mut() {
                state.show(feature, row);
            }
            Task::none()
        });
        return Task::batch([left, reading]);
    }
    let opened = open_settings(settings, ctx);
    if let Some(state) = settings.page.as_mut() {
        state.show(feature, row);
    }
    Task::batch([opened, reading])
}

/// What a page reads when it is shown: the Usage page, its log.
fn page_data(feature: SettingsFeature, ctx: &Ctx<'_>) -> Task<Message> {
    match feature {
        SettingsFeature::Usage => own_server::load_usage(ctx),
        _ => Task::none(),
    }
}
