use std::path::Path;

use iced::widget::operation;
use iced::Task;
use ncrs_config::config::{self, Config, FilingWriteError};
use ncrs_filing::filing::FilingModule;
use ncrs_i18n::Msg;
use ncrs_ui_kit::focus::release_focus;

use super::{blocking_overlay_is_open, go_to_page};
use crate::action::Action;
use crate::ctx::Ctx;
use crate::filing_pieces::TargetEdit;
use crate::hermes_overview::{OverviewState, PatternCommit};
use crate::message::{HermesFocus, HermesOverviewMessage, HermesPage, Message};
use crate::state::Hermes;
use crate::ui::filing_pieces::FILE_NAME_TEXT_FIELD_ID;
use crate::ui::hermes_overview::{FOLDER_NAME_FIELD_ID, START_OVER_CANCEL_FOCUS};

/// The writer of the config for the filing pattern of the overview.
fn write_default_pattern_alone(
    path: &Path,
    pattern: Option<&str>,
) -> Result<Box<Config>, FilingWriteError> {
    let _writing = config::one_config_writer();
    config::write_default_pattern(path, pattern).map(Box::new)
}

pub fn open(hermes: &mut Hermes, ctx: &Ctx<'_>, actions: &mut Vec<Action>) -> Task<Message> {
    if blocking_overlay_is_open(hermes, ctx) {
        return Task::none();
    }
    actions.push(Action::SetHermesFocus(HermesFocus::Content));
    hermes.return_to_overview = false;
    actions.push(Action::CloseContextMenu);
    hermes.overview = Some(OverviewState::new(
        hermes.filing_config.default_pattern.as_deref(),
        false,
    ));
    actions.push(Action::ReadOcrModelsPresent);
    actions.push(Action::RefreshFilingSnapshot);
    actions.push(Action::RequestHermesCounts);
    Task::none()
}

/// Closes the overview and gives the focus back, when it was up.
pub fn leave(hermes: &mut Hermes) -> Task<Message> {
    if hermes.overview.take().is_none() {
        return Task::none();
    }
    release_focus()
}

fn commit_pattern(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Task<Message> {
    let Some(state) = hermes.overview.as_mut() else {
        return Task::none();
    };
    let PatternCommit::Write(pattern) = state.commit_pattern() else {
        return Task::none();
    };
    let Some(config_path) = ctx.config_path.map(Path::to_path_buf) else {
        state.pattern_write_failed(ctx.lang.text(Msg::FilingSetupNoConfigPath).to_owned());
        return Task::none();
    };
    let failed_path = config_path.clone();
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                write_default_pattern_alone(&config_path, pattern.as_deref())
            })
            .await
        },
        move |joined| {
            Message::HermesOverview(HermesOverviewMessage::PatternWritten(
                joined.unwrap_or_else(|join| {
                    Err(FilingWriteError::Io {
                        path: failed_path,
                        reason: join.to_string(),
                    })
                }),
            ))
        },
    )
}

fn pattern_written(
    hermes: &mut Hermes,
    result: Result<Box<Config>, FilingWriteError>,
    write_failure: Option<String>,
    ctx: &Ctx<'_>,
) -> Task<Message> {
    match result {
        Ok(config) => {
            if let Some(state) = hermes.overview.as_mut() {
                state.pattern_written(config.filing.default_pattern.as_deref());
            }
            return commit_pattern(hermes, ctx);
        }
        Err(_) => {
            if let Some(state) = hermes.overview.as_mut() {
                state.pattern_write_failed(write_failure.unwrap_or_default());
            }
        }
    }
    Task::none()
}

fn card_covers_overview(hermes: &Hermes) -> bool {
    hermes.filing_setup.is_some() || hermes.hermes_setup.is_some()
}

/// `ChangeFolders` is answered after the app opened or closed the folders
/// card, which starts or restarts the module. `write_failure` is the words for a failed `PatternWritten`, which the
/// settings own; the app makes them before the call.
pub fn update(
    hermes: &mut Hermes,
    message: HermesOverviewMessage,
    ctx: &Ctx<'_>,
    write_failure: Option<String>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    if !matches!(message, HermesOverviewMessage::TargetEdited(_)) {
        if let Some(state) = hermes.overview.as_mut() {
            state.edit_target(TargetEdit::OwnNameCancel);
        }
    }
    match message {
        HermesOverviewMessage::Close | HermesOverviewMessage::EnterRecoveryKey => leave(hermes),
        HermesOverviewMessage::EncryptNow => {
            actions.push(Action::ChangeProtection(FilingModule::begin_encrypting));
            Task::none()
        }
        HermesOverviewMessage::AskStartOver => {
            let can_ask = hermes
                .filing
                .as_ref()
                .is_some_and(FilingModule::can_start_over)
                && !ctx.jobs_busy
                && !ctx.hermes_is_restarting;
            if can_ask {
                actions.push(Action::OpenStartOverDialog(START_OVER_CANCEL_FOCUS));
            }
            Task::none()
        }
        HermesOverviewMessage::CancelStartOver => {
            actions.push(Action::CloseStartOverDialog);
            Task::none()
        }
        HermesOverviewMessage::ConfirmStartOver => {
            if ctx.start_over_dialog_open {
                actions.push(Action::CloseStartOverDialog);
                actions.push(Action::ChangeProtection(FilingModule::begin_starting_over));
            }
            Task::none()
        }
        HermesOverviewMessage::TargetEdited(edit) => {
            let opens_name_field = edit == TargetEdit::StartOwnName;
            let opens_text_field = edit == TargetEdit::StartNameText;
            if let Some(state) = hermes.overview.as_mut() {
                state.edit_target(edit);
            }
            let write = commit_pattern(hermes, ctx);
            if opens_name_field {
                actions.push(Action::SetHermesFocus(HermesFocus::Content));
                Task::batch([operation::focus(FOLDER_NAME_FIELD_ID), write])
            } else if opens_text_field {
                actions.push(Action::SetHermesFocus(HermesFocus::Content));
                Task::batch([operation::focus(FILE_NAME_TEXT_FIELD_ID), write])
            } else {
                write
            }
        }
        HermesOverviewMessage::PatternWritten(result) => {
            pattern_written(hermes, result, write_failure, ctx)
        }
        HermesOverviewMessage::SetAiEnabled(on) => {
            actions.push(Action::SetAiEnabled(on));
            Task::none()
        }
        HermesOverviewMessage::SetLoad(load) => {
            actions.push(Action::SetLoad(load));
            Task::none()
        }
        HermesOverviewMessage::ChangeFolders => {
            hermes.overview = None;
            hermes.return_to_overview = card_covers_overview(hermes);
            Task::none()
        }
        HermesOverviewMessage::LoadOcrModels => {
            actions.push(Action::OpenOcrModels);
            Task::none()
        }
        HermesOverviewMessage::LoadAiModel => {
            actions.push(Action::OpenAndDownloadAiModel);
            Task::none()
        }
        HermesOverviewMessage::ShowOwnServerSettings => {
            hermes.overview = None;
            actions.push(Action::ShowHermesSetting(
                ctx.lang.text(Msg::SettingsRowAiProvider).to_owned(),
            ));
            Task::none()
        }
        HermesOverviewMessage::ShowProtectionSettings => {
            hermes.overview = None;
            actions.push(Action::ShowHermesSetting(
                ctx.lang
                    .text(Msg::HermesOverviewSectionProtection)
                    .to_owned(),
            ));
            Task::none()
        }
        HermesOverviewMessage::ShowReviews => {
            if ctx.hermes_has_open_reviews {
                go_to_page(hermes, HermesPage::Review, ctx, actions)
            } else {
                Task::none()
            }
        }
        HermesOverviewMessage::SwitchHermesOn => {
            if hermes.filing_config.locations.is_empty() {
                hermes.overview = None;
                actions.push(Action::OpenHermesSetup);
                hermes.return_to_overview = true;
            }
            Task::none()
        }
        HermesOverviewMessage::ContinueSetup => {
            hermes.overview = None;
            actions.push(Action::OpenHermesSetup);
            hermes.return_to_overview = true;
            Task::none()
        }
        HermesOverviewMessage::TryAgain => {
            actions.push(Action::RestartHermesModule);
            Task::none()
        }
    }
}
