use super::*;
use crate::messages::OcrModelsMessage;
use crate::ocr_models::{DownloadError, OcrModelsCard};
use ncrs_intake::OCR_MODEL_FILES;
use std::path::PathBuf;

fn card() -> OcrModelsCard {
    OcrModelsCard::new(Some(PathBuf::from("/example/data/ocr-models")))
}

fn shows(card: OcrModelsCard, label: &str) -> bool {
    simulator(&App::with_ocr_models_card(card))
        .find(label)
        .is_ok()
}

fn messages_after_click(card: OcrModelsCard, label: &str) -> Vec<Message> {
    let app = App::with_ocr_models_card(card);
    let mut sim = simulator(&app);
    sim.click(label).expect("the clicked label");
    sim.into_messages().filter(is_action).collect()
}

#[test]
fn the_card_names_host_files_sizes_hashes_and_folder_before_anything_is_sent() {
    let mut labels = vec![
        "Download from: ocrs-models.s3-accelerate.amazonaws.com".to_owned(),
        "Saved to: /example/data/ocr-models".to_owned(),
        "The models are made by the ocrs project: github.com/robertknight/ocrs-models".to_owned(),
    ];
    for file in &OCR_MODEL_FILES {
        labels.push(format!(
            "{}: {}",
            file.name,
            crate::ui::format::size(file.size)
        ));
        labels.push(format!("SHA-256 checksum: {}", file.sha256));
    }
    for label in labels {
        assert!(shows(card(), &label), "{label}");
    }
}

#[test]
fn the_buttons_send_close_and_download_and_nothing_else() {
    assert_eq!(
        messages_after_click(card(), "Cancel"),
        [Message::OcrModels(OcrModelsMessage::Close)]
    );
    assert_eq!(
        messages_after_click(card(), "Download"),
        [Message::OcrModels(OcrModelsMessage::Download)]
    );
}

#[test]
fn while_it_downloads_the_card_says_so_and_the_buttons_are_off() {
    let mut running = card();
    running.start().unwrap();
    assert!(shows(running.clone(), "Downloading…"));
    assert!(messages_after_click(running, "Download").is_empty());
}

#[test]
fn a_failure_is_shown_with_its_detail() {
    let mut failed = card();
    failed.start().unwrap();
    failed.finish(Err(DownloadError::Network(
        crate::open_vsx::FetchError::Timeout,
    )));
    assert!(shows(
        failed,
        "Download failed (the server did not answer in time)"
    ));
}

#[test]
fn the_card_is_worded_in_german_too() {
    let app = App::with_ocr_models_card(card()).in_language(Language::German);
    let mut sim = simulator(&app);
    for label in [
        "Modelle für Texterkennung laden",
        "Herunterladen von: ocrs-models.s3-accelerate.amazonaws.com",
        "Gespeichert in: /example/data/ocr-models",
        "Abbrechen",
        "Laden",
    ] {
        assert!(sim.find(label).is_ok(), "{label}");
    }
}

fn matches_snapshot(language: Language, name: &str) {
    let app = App::with_ocr_models_card(card()).in_language(language);
    let matches = simulator(&app)
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image(format!("tests/snapshots/{name}.png"))
        .expect("snapshot comparison");
    assert!(
        matches,
        "the models card does not match tests/snapshots/{name}.png"
    );
}

#[test]
fn the_card_matches_its_snapshots() {
    matches_snapshot(Language::English, "ocr_models_en");
    matches_snapshot(Language::German, "ocr_models_de");
}
