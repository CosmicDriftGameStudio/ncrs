use std::path::Path;

use iced::Task;

use crate::action::Action;
use crate::ai_model::view::CANCEL_FOCUS as AI_CANCEL_FOCUS;
use crate::ai_model::{
    model_is_installed, AiModelCard, DownloadError as AiDownloadError, InstallOutcome,
};
use crate::ctx::Ctx;
use crate::message::{AiModelMessage, Message, OcrModelsMessage, SettingChange};
use crate::ocr_models::view::CANCEL_FOCUS as OCR_CANCEL_FOCUS;
use crate::ocr_models::{DownloadError, OcrModelsCard};
use crate::open_vsx::FetchError;
use crate::state::Settings;
use crate::write::write_setting;

pub fn update_ocr(
    settings: &mut Settings,
    message: OcrModelsMessage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match message {
        OcrModelsMessage::Open => open_ocr(settings, ctx, actions),
        OcrModelsMessage::Close => close_ocr(settings),
        OcrModelsMessage::Download => start_ocr_download(settings, ctx),
        OcrModelsMessage::Progress => Task::none(),
        OcrModelsMessage::Downloaded { generation, result } => {
            ocr_downloaded(settings, generation, result, actions)
        }
    }
}

/// Only asks. Nothing is sent before the download button is pressed.
fn open_ocr(settings: &mut Settings, ctx: &Ctx<'_>, actions: &mut Vec<Action>) -> Task<Message> {
    if ctx.overlay_apart_from_overview_open {
        return Task::none();
    }
    actions.push(Action::CloseContextMenu);
    settings.ocr_models_generation = settings.ocr_models_generation.wrapping_add(1);
    actions.push(Action::SetDialogFocus(OCR_CANCEL_FOCUS));
    let target = ctx.ocr_models_dir.clone();
    settings.ocr_models = Some(if ocr_models_present(ctx.ocr_models_dir.as_deref()) {
        OcrModelsCard::installed(target)
    } else {
        OcrModelsCard::new(target)
    });
    Task::none()
}

/// Whether the folder of the OCR models has files in it.
pub fn ocr_models_present(models_dir: Option<&Path>) -> bool {
    models_dir.is_some_and(|dir| dir.read_dir().is_ok_and(|mut files| files.next().is_some()))
}

/// Not while the files are written: closing would leave a download nobody
/// reports on.
fn close_ocr(settings: &mut Settings) -> Task<Message> {
    if settings.ocr_models_download_runs() {
        return Task::none();
    }
    settings.ocr_models = None;
    Task::none()
}

fn start_ocr_download(settings: &mut Settings, ctx: &Ctx<'_>) -> Task<Message> {
    let Some(target) = settings.ocr_models.as_mut().and_then(OcrModelsCard::start) else {
        return Task::none();
    };
    let received = settings.ocr_models.as_ref().map(OcrModelsCard::received);
    let generation = settings.ocr_models_generation;
    let download = ctx.services.download_ocr_models;
    Task::perform(
        tokio::task::spawn_blocking(move || download(&target, &received.unwrap_or_default())),
        move |joined| {
            let result = joined.unwrap_or_else(|join| {
                Err(DownloadError::Network(FetchError::Network(
                    join.to_string(),
                )))
            });
            Message::OcrModels(OcrModelsMessage::Downloaded { generation, result })
        },
    )
}

fn ocr_downloaded(
    settings: &mut Settings,
    generation: u64,
    result: Result<(), DownloadError>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    if generation != settings.ocr_models_generation {
        return Task::none();
    }
    let Some(card) = settings.ocr_models.as_mut() else {
        return Task::none();
    };
    if !card.finish(result) {
        return Task::none();
    }
    if let Some(page) = settings.page.as_mut() {
        page.ocr_models_present = true;
    }
    actions.push(Action::OcrModelsInstalled);
    Task::none()
}

pub fn update_ai(
    settings: &mut Settings,
    message: AiModelMessage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match message {
        AiModelMessage::Progress => Task::none(),
        AiModelMessage::ShowAiSetting => {
            settings.ai_model = None;
            actions.push(Action::OpenOverview);
            Task::none()
        }
        AiModelMessage::Open => open_ai(settings, ctx, actions),
        AiModelMessage::Close => close_ai(settings, ctx),
        AiModelMessage::Download => start_ai_download(settings, ctx),
        AiModelMessage::Stop => {
            if let Some(card) = settings.ai_model.as_mut() {
                card.stop();
            }
            Task::none()
        }
        AiModelMessage::Downloaded { generation, result } => {
            ai_downloaded(settings, generation, result, ctx, actions)
        }
    }
}

fn open_ai(settings: &mut Settings, ctx: &Ctx<'_>, actions: &mut Vec<Action>) -> Task<Message> {
    if ctx.overlay_apart_from_overview_open {
        return Task::none();
    }
    actions.push(Action::CloseContextMenu);
    settings.ai_card_hidden = false;
    settings.ai_model_generation = settings.ai_model_generation.wrapping_add(1);
    actions.push(Action::SetDialogFocus(AI_CANCEL_FOCUS));
    let target = ctx.ai_models_dir.clone();
    let has_partial = target
        .as_deref()
        .is_some_and(|dir| ncrs_ai::has_partial_download(dir, &ncrs_ai::LOCAL_MODEL));
    settings.ai_model = Some(if target.as_deref().is_some_and(model_is_installed) {
        AiModelCard::installed(target)
    } else {
        AiModelCard::new(target, has_partial)
    });
    Task::none()
}

/// Not while the file is written: stopping comes first, then closing.
fn close_ai(settings: &mut Settings, ctx: &Ctx<'_>) -> Task<Message> {
    if settings.ai_model_download_runs() {
        settings.ai_card_hidden = ctx.overview_open;
        return Task::none();
    }
    settings.ai_model = None;
    settings.ai_card_hidden = false;
    Task::none()
}

pub fn press_ai_model_button(
    settings: &mut Settings,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    if settings.ai_model_download_runs() {
        return update_ai(settings, AiModelMessage::Stop, ctx, actions);
    }
    start_ai_download(settings, ctx)
}

fn start_ai_download(settings: &mut Settings, ctx: &Ctx<'_>) -> Task<Message> {
    let Some(job) = settings.ai_model.as_mut().and_then(AiModelCard::start) else {
        return Task::none();
    };
    let generation = settings.ai_model_generation;
    let download = ctx.services.download_ai_model;
    Task::perform(
        tokio::task::spawn_blocking(move || download(&job)),
        move |joined| {
            let result = joined.unwrap_or_else(|join| {
                Err(AiDownloadError::Network(FetchError::Network(
                    join.to_string(),
                )))
            });
            Message::AiModel(AiModelMessage::Downloaded { generation, result })
        },
    )
}

fn ai_downloaded(
    settings: &mut Settings,
    generation: u64,
    result: Result<(), AiDownloadError>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    if generation != settings.ai_model_generation {
        return Task::none();
    }
    let Some(card) = settings.ai_model.as_mut() else {
        return Task::none();
    };
    if !card.finish(result) {
        return Task::none();
    }
    settings.ai_card_hidden = false;
    actions.push(Action::SetDialogFocus(AI_CANCEL_FOCUS));
    actions.push(Action::RefreshAiModel);
    let can_write = ctx.config_path.is_some();
    let (outcome, task) = if ctx.ai_is_on {
        (InstallOutcome::InUse, Task::none())
    } else if can_write {
        (
            InstallOutcome::TurningOn,
            write_setting(settings, SettingChange::AiEnabled(true), ctx),
        )
    } else {
        (InstallOutcome::StillOff, Task::none())
    };
    if let Some(open_card) = settings.ai_model.as_mut() {
        open_card.set_outcome(outcome);
    }
    task
}
