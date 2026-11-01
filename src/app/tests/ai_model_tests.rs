use std::sync::atomic::{AtomicUsize, Ordering};

use crate::app::*;

static DOWNLOADS: AtomicUsize = AtomicUsize::new(0);

fn counting_download(_: &DownloadJob) -> Result<(), AiDownloadError> {
    DOWNLOADS.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

fn quiet_download(_: &DownloadJob) -> Result<(), AiDownloadError> {
    Ok(())
}

fn send(app: &mut App, message: AiModelMessage) {
    drop(app.update(Message::AiModel(message)));
}

fn app_with_data_folder() -> App {
    let mut app = App::with_fixed_panels();
    app.hermes.index_path = Some(PathBuf::from("/example/data/index.db"));
    app.download_ai_model = quiet_download;
    app
}

#[test]
fn the_palette_entry_opens_the_card_and_nothing_is_downloaded() {
    let before = DOWNLOADS.load(Ordering::SeqCst);
    let mut app = app_with_data_folder();
    app.download_ai_model = counting_download;
    drop(app.update(Message::CommandPalette(CommandPaletteMessage::Toggle)));
    drop(app.update(Message::CommandPalette(
        CommandPaletteMessage::QueryChanged("Download AI model".to_owned()),
    )));
    drop(app.update(Message::CommandPalette(CommandPaletteMessage::Run)));

    let card = app.ai_model_for_test().unwrap();
    assert_eq!(
        card.target(),
        Some(Path::new("/example/data/ai-models")),
        "the folder next to the index"
    );
    assert!(!card.is_downloading());
    assert!(app.blocking_overlay_is_open());
    assert_eq!(DOWNLOADS.load(Ordering::SeqCst), before);
}

#[test]
fn escape_closes_the_card_and_enter_presses_cancel() {
    let mut app = app_with_data_folder();
    send(&mut app, AiModelMessage::Open);
    let route = |current: &App, key: Named| {
        route_key(
            &current.prompt_key_state(),
            Key::Named(key),
            Modifiers::default(),
        )
    };
    assert_eq!(
        route(&app, Named::Escape),
        Some(Message::AiModel(AiModelMessage::Close))
    );
    drop(app.update(Message::DialogActivate));
    assert!(app.ai_model_for_test().is_none());
}

#[tokio::test]
async fn the_download_button_starts_one_download_and_closing_waits() {
    let mut app = app_with_data_folder();
    send(&mut app, AiModelMessage::Open);
    send(&mut app, AiModelMessage::Download);
    assert!(app.ai_model_for_test().unwrap().is_downloading());
    send(&mut app, AiModelMessage::Close);
    assert!(app.ai_model_for_test().is_some());
}

#[tokio::test]
async fn a_finished_download_keeps_the_card_and_says_what_happened() {
    let mut app = app_with_data_folder();
    send(&mut app, AiModelMessage::Open);
    send(&mut app, AiModelMessage::Download);
    let generation = app.settings.ai_model_generation;
    send(
        &mut app,
        AiModelMessage::Downloaded {
            generation,
            result: Ok(()),
        },
    );
    let card = app.ai_model_for_test().unwrap();
    assert!(card.is_finished(), "the card stays and says what happened");
    assert_eq!(
        card.outcome(),
        Some(InstallOutcome::StillOff),
        "the config of this app cannot be written, so the AI stays off"
    );
}

#[tokio::test]
async fn the_bar_ticks_only_while_the_download_runs() {
    let mut app = app_with_data_folder();
    assert!(!app.settings.ai_model_download_runs());
    send(&mut app, AiModelMessage::Open);
    assert!(
        !app.settings.ai_model_download_runs(),
        "an open card alone is idle"
    );
    send(&mut app, AiModelMessage::Download);
    assert!(app.settings.ai_model_download_runs());
    let generation = app.settings.ai_model_generation;
    send(
        &mut app,
        AiModelMessage::Downloaded {
            generation,
            result: Ok(()),
        },
    );
    assert!(
        !app.settings.ai_model_download_runs(),
        "a finished card needs no ticks"
    );
}

#[tokio::test]
async fn a_finished_download_turns_the_ai_on_when_the_config_can_be_written() {
    let mut app = app_with_data_folder();
    app.config_path = Some(PathBuf::from("/example/config.toml"));
    send(&mut app, AiModelMessage::Open);
    send(&mut app, AiModelMessage::Download);
    let generation = app.settings.ai_model_generation;
    send(
        &mut app,
        AiModelMessage::Downloaded {
            generation,
            result: Ok(()),
        },
    );
    assert_eq!(
        app.ai_model_for_test().unwrap().outcome(),
        Some(InstallOutcome::TurningOn)
    );
}

#[test]
fn an_answer_for_a_closed_card_is_dropped() {
    let mut app = app_with_data_folder();
    send(&mut app, AiModelMessage::Open);
    let stale = app.settings.ai_model_generation;
    send(&mut app, AiModelMessage::Close);
    send(&mut app, AiModelMessage::Open);
    send(
        &mut app,
        AiModelMessage::Downloaded {
            generation: stale,
            result: Ok(()),
        },
    );
    assert!(app.ai_model_for_test().is_some());
    assert_eq!(app.notice_for_test(), None);
}

#[test]
fn without_a_data_folder_the_button_says_why() {
    let mut app = App::with_fixed_panels();
    app.hermes.index_path = None;
    send(&mut app, AiModelMessage::Open);
    send(&mut app, AiModelMessage::Download);
    let card = app.ai_model_for_test().unwrap();
    assert_eq!(card.error(), Some(&AiDownloadError::NoDataFolder));
    assert!(!card.is_downloading());
}
