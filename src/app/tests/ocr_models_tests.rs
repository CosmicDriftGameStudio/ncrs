use std::sync::atomic::{AtomicUsize, Ordering};

use crate::app::*;

/// Only the tests that never press Download use the counting one.
static DOWNLOADS: AtomicUsize = AtomicUsize::new(0);

fn counting_download(_: &Path, _: &AtomicU64) -> Result<(), DownloadError> {
    DOWNLOADS.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

fn quiet_download(_: &Path, _: &AtomicU64) -> Result<(), DownloadError> {
    Ok(())
}

fn send(app: &mut App, message: OcrModelsMessage) {
    drop(app.update(Message::OcrModels(message)));
}

fn app_with_data_folder() -> App {
    let mut app = App::with_fixed_panels();
    app.hermes.index_path = Some(PathBuf::from("/example/data/index.db"));
    app.download_ocr_models = quiet_download;
    app
}

fn route(app: &App, key: Named) -> Option<Message> {
    route_key(
        &app.prompt_key_state(),
        Key::Named(key),
        Modifiers::default(),
    )
}

#[test]
fn the_palette_entry_opens_the_card_and_nothing_is_downloaded() {
    let before = DOWNLOADS.load(Ordering::SeqCst);
    let mut app = app_with_data_folder();
    app.download_ocr_models = counting_download;
    drop(app.update(Message::CommandPalette(CommandPaletteMessage::Toggle)));
    drop(app.update(Message::CommandPalette(
        CommandPaletteMessage::QueryChanged("text recognition".to_owned()),
    )));
    drop(app.update(Message::CommandPalette(CommandPaletteMessage::Run)));

    let card = app.ocr_models_for_test().unwrap();
    assert_eq!(
        card.target(),
        Some(Path::new("/example/data/ocr-models")),
        "the folder next to the index"
    );
    assert!(!card.is_downloading());
    assert!(app.blocking_overlay_is_open());
    assert_eq!(DOWNLOADS.load(Ordering::SeqCst), before);
}

#[test]
fn cancel_and_escape_close_the_card_without_a_download() {
    let before = DOWNLOADS.load(Ordering::SeqCst);
    let mut app = app_with_data_folder();
    app.download_ocr_models = counting_download;
    send(&mut app, OcrModelsMessage::Open);
    assert_eq!(
        route(&app, Named::Escape),
        Some(Message::OcrModels(OcrModelsMessage::Close))
    );
    assert_eq!(
        route(&app, Named::Enter),
        Some(Message::DialogActivate),
        "Enter presses the focused button"
    );

    drop(app.update(Message::DialogActivate));
    assert!(
        app.ocr_models_for_test().is_none(),
        "Cancel has the focus when it opens"
    );
    assert_eq!(DOWNLOADS.load(Ordering::SeqCst), before);
}

#[tokio::test]
async fn the_download_button_starts_one_download_only() {
    let mut app = app_with_data_folder();
    send(&mut app, OcrModelsMessage::Open);
    send(&mut app, OcrModelsMessage::Download);
    assert!(app.ocr_models_for_test().unwrap().is_downloading());

    send(&mut app, OcrModelsMessage::Close);
    assert!(
        app.ocr_models_for_test().is_some(),
        "closing waits while the files are written"
    );
}

#[tokio::test]
async fn a_finished_download_keeps_the_card_and_says_what_happened() {
    let mut app = app_with_data_folder();
    send(&mut app, OcrModelsMessage::Open);
    send(&mut app, OcrModelsMessage::Download);
    let generation = app.settings.ocr_models_generation;
    send(
        &mut app,
        OcrModelsMessage::Downloaded {
            generation,
            result: Ok(()),
        },
    );
    let card = app.ocr_models_for_test().unwrap();
    assert!(card.is_finished(), "the card stays and says what happened");
    assert_eq!(card.progress(), None, "the bar goes away with the download");
}

#[tokio::test]
async fn the_ocr_bar_ticks_only_while_the_download_runs() {
    let mut app = app_with_data_folder();
    send(&mut app, OcrModelsMessage::Open);
    assert!(!app.settings.ocr_models_download_runs());
    send(&mut app, OcrModelsMessage::Download);
    assert!(app.settings.ocr_models_download_runs());
    let generation = app.settings.ocr_models_generation;
    send(
        &mut app,
        OcrModelsMessage::Downloaded {
            generation,
            result: Ok(()),
        },
    );
    assert!(!app.settings.ocr_models_download_runs());
}

#[tokio::test]
async fn a_failed_download_stays_on_the_card_and_can_be_tried_again() {
    let mut app = app_with_data_folder();
    send(&mut app, OcrModelsMessage::Open);
    send(&mut app, OcrModelsMessage::Download);
    let generation = app.settings.ocr_models_generation;
    send(
        &mut app,
        OcrModelsMessage::Downloaded {
            generation,
            result: Err(DownloadError::Network(FetchError::Timeout)),
        },
    );
    let card = app.ocr_models_for_test().unwrap();
    assert!(!card.is_downloading());
    assert_eq!(
        card.error(),
        Some(&DownloadError::Network(FetchError::Timeout))
    );
    send(&mut app, OcrModelsMessage::Download);
    assert!(app.ocr_models_for_test().unwrap().is_downloading());
}

#[test]
fn an_answer_for_a_closed_card_is_dropped() {
    let mut app = app_with_data_folder();
    send(&mut app, OcrModelsMessage::Open);
    let stale = app.settings.ocr_models_generation;
    send(&mut app, OcrModelsMessage::Close);
    send(&mut app, OcrModelsMessage::Open);
    send(
        &mut app,
        OcrModelsMessage::Downloaded {
            generation: stale,
            result: Ok(()),
        },
    );
    assert!(app.ocr_models_for_test().is_some());
    assert_eq!(app.notice_for_test(), None);
}

#[test]
fn without_a_data_folder_the_button_says_why() {
    let mut app = App::with_fixed_panels();
    app.hermes.index_path = None;
    send(&mut app, OcrModelsMessage::Open);
    send(&mut app, OcrModelsMessage::Download);
    let card = app.ocr_models_for_test().unwrap();
    assert_eq!(card.error(), Some(&DownloadError::NoDataFolder));
    assert!(!card.is_downloading());
}

#[test]
fn it_is_not_opened_over_another_modal() {
    let mut app = app_with_data_folder();
    drop(app.update(Message::CommandPalette(CommandPaletteMessage::Toggle)));
    send(&mut app, OcrModelsMessage::Open);
    assert!(app.ocr_models_for_test().is_none());
}
