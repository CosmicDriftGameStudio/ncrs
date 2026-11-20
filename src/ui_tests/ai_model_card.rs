use super::*;
use crate::ai_model::AiModelCard;
use crate::messages::AiModelMessage;
use std::path::PathBuf;

fn card() -> AiModelCard {
    AiModelCard::new(Some(PathBuf::from("/example/data/ai-models")), false)
}

fn shows(card: AiModelCard, label: &str) -> bool {
    simulator(&App::with_ai_model_card(card))
        .find(label)
        .is_ok()
}

fn messages_after_click(card: AiModelCard, label: &str) -> Vec<Message> {
    let app = App::with_ai_model_card(card);
    let mut sim = simulator(&app);
    sim.click(label).expect("the clicked label");
    sim.into_messages().collect()
}

#[test]
fn the_card_names_host_file_size_hash_license_and_folder_before_anything_is_sent() {
    let pin = ncrs_ai::LOCAL_MODEL;
    for label in [
        "Download from: huggingface.co".to_owned(),
        format!("{}: {}", pin.name, crate::ui::format::megabytes(pin.size)),
        format!("SHA-256 checksum: {}", pin.sha256),
        "License: Apache-2.0".to_owned(),
        "Saved to: /example/data/ai-models".to_owned(),
    ] {
        assert!(shows(card(), &label), "{label}");
    }
}

#[test]
fn the_buttons_send_close_and_download_and_nothing_else() {
    assert_eq!(
        messages_after_click(card(), "Cancel"),
        [Message::AiModel(AiModelMessage::Close)]
    );
    assert_eq!(
        messages_after_click(card(), "Download"),
        [Message::AiModel(AiModelMessage::Download)]
    );
}

#[test]
fn a_running_download_has_only_the_stop_button() {
    let mut running = card();
    running.start().unwrap();
    assert!(shows(running.clone(), "Downloading…"));
    assert!(!shows(running.clone(), "Cancel"));
    assert_eq!(
        messages_after_click(running, "Stop"),
        [Message::AiModel(AiModelMessage::Stop)]
    );
}

#[test]
fn a_partial_file_offers_to_continue() {
    let partial = AiModelCard::new(Some(PathBuf::from("/example/data/ai-models")), true);
    assert_eq!(
        messages_after_click(partial, "Continue download"),
        [Message::AiModel(AiModelMessage::Download)]
    );
}

#[test]
fn the_card_is_worded_in_german_too() {
    let app = App::with_ai_model_card(card()).in_language(Language::German);
    let mut sim = simulator(&app);
    for label in [
        "KI-Modell laden",
        "Herunterladen von: huggingface.co",
        "Gespeichert in: /example/data/ai-models",
        "Lizenz: Apache-2.0",
        "Abbrechen",
        "Laden",
    ] {
        assert!(sim.find(label).is_ok(), "{label}");
    }
}

fn matches_snapshot(language: Language, name: &str) {
    let app = App::with_ai_model_card(card()).in_language(language);
    let matches = simulator(&app)
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image(format!("tests/snapshots/{name}.png"))
        .expect("snapshot comparison");
    assert!(
        matches,
        "the AI model card does not match tests/snapshots/{name}.png"
    );
}

#[test]
fn the_card_matches_its_snapshots() {
    matches_snapshot(Language::English, "ai_model_en");
    matches_snapshot(Language::German, "ai_model_de");
}

fn matches_card_snapshot(card: AiModelCard, language: Language, name: &str) {
    let app = App::with_ai_model_card(card).in_language(language);
    let matches = simulator(&app)
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_reference_image(format!("tests/snapshots/{name}.png"))
        .expect("snapshot comparison");
    assert!(
        matches,
        "the card does not match tests/snapshots/{name}.png"
    );
}

fn downloading() -> AiModelCard {
    let mut running = card();
    running.start().unwrap();
    running.set_received_for_test(312_000_000);
    running
}

fn finished(outcome: crate::ai_model::InstallOutcome) -> AiModelCard {
    let mut done = card();
    done.start().unwrap();
    assert!(done.finish(Ok(())));
    done.set_outcome(outcome);
    done
}

#[test]
fn a_running_download_shows_its_bar_and_the_figures_and_only_stop() {
    assert!(shows(
        downloading(),
        &format!(
            "312 / {} · 11 %",
            crate::ui::format::megabytes(ncrs_ai::LOCAL_MODEL.size)
        )
    ));
    assert!(!shows(downloading(), "Download"));
    matches_card_snapshot(downloading(), Language::English, "ai_model_downloading_en");
    matches_card_snapshot(downloading(), Language::German, "ai_model_downloading_de");
}

#[test]
fn a_finished_download_says_what_became_of_the_ai_switch() {
    use crate::ai_model::InstallOutcome;
    assert!(shows(
        finished(InstallOutcome::TurnedOn),
        "The AI model is installed. AI is now on: ncrs fills in fields for which there is no rule yet."
    ));
    assert_eq!(
        messages_after_click(finished(InstallOutcome::TurnedOn), "Done"),
        [Message::AiModel(AiModelMessage::Close)]
    );
    assert_eq!(
        messages_after_click(finished(InstallOutcome::StillOff), "Turn on AI"),
        [Message::AiModel(AiModelMessage::TurnOn)]
    );
    matches_card_snapshot(
        finished(InstallOutcome::TurnedOn),
        Language::English,
        "ai_model_finished_en",
    );
    matches_card_snapshot(
        finished(InstallOutcome::TurnedOn),
        Language::German,
        "ai_model_finished_de",
    );
}

#[test]
fn a_model_that_is_already_there_shows_installed_and_only_close() {
    let installed = AiModelCard::installed(Some(PathBuf::from("/example/data/ai-models")));
    assert!(shows(installed.clone(), "Installed"));
    assert!(!shows(installed.clone(), "Download"));
    assert!(!shows(installed.clone(), "Download from: huggingface.co"));
    assert_eq!(
        messages_after_click(installed.clone(), "Close"),
        [Message::AiModel(AiModelMessage::Close)]
    );
    matches_card_snapshot(
        installed.clone(),
        Language::English,
        "ai_model_installed_en",
    );
    matches_card_snapshot(installed, Language::German, "ai_model_installed_de");
}
