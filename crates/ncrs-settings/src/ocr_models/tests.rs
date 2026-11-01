use super::*;

fn card() -> OcrModelsCard {
    OcrModelsCard::new(Some(PathBuf::from("data/ocr-models")))
}

#[test]
fn the_card_starts_only_on_the_button_and_only_once() {
    let mut card = card();
    assert!(!card.is_downloading());
    assert_eq!(card.start(), Some(PathBuf::from("data/ocr-models")));
    assert!(card.is_downloading());
    assert_eq!(card.start(), None, "no second download");
}

#[test]
fn without_a_data_folder_nothing_starts_and_the_reason_is_shown() {
    let mut card = OcrModelsCard::new(None);
    assert_eq!(card.start(), None);
    assert!(!card.is_downloading());
    assert_eq!(card.error(), Some(&DownloadError::NoDataFolder));
}

#[test]
fn a_failed_download_keeps_the_card_with_the_reason_and_can_be_tried_again() {
    let mut card = card();
    card.start();
    let error = DownloadError::Install(OcrModelError::WrongHash {
        file: "text-detection.onnx",
    });
    assert!(!card.finish(Err(error.clone())));
    assert_eq!(card.error(), Some(&error));
    assert!(!card.is_downloading());
    assert!(card.start().is_some());
    assert_eq!(card.error(), None, "a new try ends the complaint");
}

#[test]
fn a_finished_download_closes_the_card() {
    let mut card = card();
    card.start();
    assert!(card.finish(Ok(())));
}

#[test]
fn an_answer_nobody_waits_for_changes_nothing() {
    let mut card = card();
    assert!(!card.finish(Ok(())));
    assert!(!card.finish(Err(DownloadError::NoDataFolder)));
    assert_eq!(card.error(), None);
}

#[test]
fn only_the_host_of_the_models_passes_the_check() {
    for file in OCR_MODEL_FILES {
        assert_eq!(open_vsx::check_host_in(&file.url(), &MODEL_HOSTS), Ok(()));
    }
    for url in [
        "http://ocrs-models.s3-accelerate.amazonaws.com/a",
        "https://ocrs-models.s3.amazonaws.com/a",
        "https://ocrs-models.s3-accelerate.amazonaws.com.evil.example/a",
        "https://user@ocrs-models.s3-accelerate.amazonaws.com/a",
        "https://open-vsx.org/a",
    ] {
        assert!(
            matches!(
                open_vsx::check_host_in(url, &MODEL_HOSTS),
                Err(FetchError::DownloadRefused { .. })
            ),
            "{url}"
        );
    }
}
