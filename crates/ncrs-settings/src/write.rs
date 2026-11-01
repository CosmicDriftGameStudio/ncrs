use std::path::Path;

use iced::Task;
use ncrs_config::config::{self, Config, FilingWriteError, KeyChange};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::format::shown_path;

use crate::ctx::Ctx;
use crate::message::{Message, SettingChange, SettingsMessage};
use crate::state::Settings;

/// The writer of the config for an edit in the keyboard shortcuts dialog.
pub fn write_key_changes(
    path: &Path,
    changes: &[KeyChange],
) -> Result<Box<Config>, FilingWriteError> {
    let _writing = config::one_config_writer();
    config::write_keys(path, changes).map(Box::new)
}

/// The writer of the config for one change of the settings tab.
pub fn write_setting_change(
    path: &Path,
    change: SettingChange,
) -> Result<Box<Config>, FilingWriteError> {
    let _writing = config::one_config_writer();
    let written = match change {
        SettingChange::Preset(preset) => config::write_theme_preset(path, preset),
        SettingChange::Language(lang) => config::write_language(path, lang),
        SettingChange::HermesEnabled(enabled) => config::write_filing_enabled(path, enabled),
        SettingChange::Load(load) => config::write_filing_load(path, load),
        SettingChange::AiEnabled(enabled) => config::write_ai_enabled(path, enabled),
        SettingChange::Program { kind, line } => config::write_program(path, kind, line.as_ref()),
        SettingChange::VscodeTheme(Some(file)) => config::write_vscode_theme(path, &file),
        SettingChange::VscodeTheme(None) => config::remove_vscode_theme(path),
        SettingChange::Color { role, color } => config::write_color(path, role, color.as_deref()),
        SettingChange::AiProvider(choice) => config::write_ai_provider(path, &choice),
        SettingChange::Servers(servers) => config::write_servers(path, &servers),
        SettingChange::OwnDetails { detail, entries } => {
            config::write_own_details(path, detail, &entries)
        }
        SettingChange::FileListTags(tags) => config::write_file_list_tags(path, &tags),
    };
    written.map(Box::new)
}

/// Writes `change` to the config on a blocking thread; the answer is a
/// `Written` with the file read back, and only then it is applied.
pub fn write_setting(
    settings: &mut Settings,
    change: SettingChange,
    ctx: &Ctx<'_>,
) -> Task<Message> {
    let Some(config_path) = ctx.config_path.map(Path::to_path_buf) else {
        settings.failure = Some(ctx.lang.text(Msg::FilingSetupNoConfigPath).to_owned());
        return Task::none();
    };
    settings.failure = None;
    let written_path = config_path.clone();
    let written_change = change.clone();
    // Started only when the task runs, so writes chained one after the
    // other do not read the file at the same time.
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || write_setting_change(&written_path, written_change))
                .await
        },
        move |joined| {
            Message::Settings(SettingsMessage::Written {
                change,
                result: joined.unwrap_or_else(|join| {
                    Err(FilingWriteError::Io {
                        path: config_path,
                        reason: join.to_string(),
                    })
                }),
            })
        },
    )
}

/// The words for a write that failed.
pub fn write_failure(error: &FilingWriteError, ctx: &Ctx<'_>) -> String {
    let lang = ctx.lang;
    let shown = |path: &Path| shown_path(path, ctx.home);
    let write_failed = |path: &Path, reason: &str| {
        fill_template(
            lang.text(Msg::FilingSetupWriteFailed),
            &[("{path}", &shown(path)), ("{reason}", reason)],
        )
    };
    match error {
        FilingWriteError::Io { path, reason } => write_failed(path, reason),
        FilingWriteError::PathNotText(path) => fill_template(
            lang.text(Msg::FilingSetupPathNotText),
            &[("{path}", &shown(path))],
        ),
        FilingWriteError::Config(config_error) => {
            let path = shown(config_error.path());
            match config_error.line() {
                Some(line) => fill_template(
                    lang.text(Msg::FilingSetupConfigBrokenAtLine),
                    &[
                        ("{path}", &path),
                        ("{line}", &line.to_string()),
                        ("{reason}", config_error.reason()),
                    ],
                ),
                None => fill_template(
                    lang.text(Msg::FilingSetupConfigBroken),
                    &[("{path}", &path), ("{reason}", config_error.reason())],
                ),
            }
        }
        // The writers of the tab check no filing rules.
        FilingWriteError::Rule(violation) => write_failed(
            ctx.config_path.unwrap_or(Path::new("")),
            &format!("{violation:?}"),
        ),
    }
}
