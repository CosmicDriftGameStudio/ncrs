//! The chat's AI offers write the same setting as the overview checkbox.

use crate::app::*;
use iced_test::futures::futures::StreamExt as _;
use iced_test::runtime::{task, Action};
use ncrs_ai::{LocalModelStatus, ScriptedChatModel};

use crate::config::AiSetting;

fn quiet_download(_: &DownloadJob) -> Result<(), AiDownloadError> {
    Ok(())
}

/// Delivers `message` and everything the tasks it starts report.
async fn settle(app: &mut App, message: Message) {
    let first = app.update(message);
    settle_task(app, first).await;
}

async fn settle_task(app: &mut App, first: Task<Message>) {
    let mut queue = vec![first];
    while let Some(next) = queue.pop() {
        let Some(mut stream) = task::into_stream(next) else {
            continue;
        };
        while let Some(action) = stream.next().await {
            if let Action::Output(reported) = action {
                if !matches!(reported, Message::NoticeExpired(_)) {
                    queue.push(app.update(reported));
                }
            }
        }
    }
}

fn chat_with_ai_off(config_dir: &tempfile::TempDir) -> App {
    let mut app = App::with_fixed_panels().in_language(Language::German);
    app.hermes.filing = Some(FilingModule::in_phase(Phase::Ready));
    let config_path = config_dir.path().join("config.toml");
    std::fs::write(&config_path, "").unwrap();
    app.config_path = Some(config_path);
    app
}

fn config_text(config_dir: &tempfile::TempDir) -> String {
    std::fs::read_to_string(config_dir.path().join("config.toml")).unwrap()
}

fn chat_chip(app: &App, reply: &ncrs_hermes_chat::ChipReply) -> Message {
    let chip = app
        .hermes_chat
        .chips()
        .iter()
        .find(|chip| &chip.reply == reply)
        .unwrap_or_else(|| panic!("no chip {reply:?}"));
    Message::HermesChat(HermesChatMessage::ChipPressed {
        label: chip.label.clone(),
        reply: chip.reply.clone(),
    })
}

#[tokio::test]
async fn the_turn_on_chip_writes_the_ai_setting_and_the_chat_goes_on() {
    let config_dir = tempfile::tempdir().unwrap();
    let mut app = chat_with_ai_off(&config_dir);
    settle(&mut app, Message::HermesChat(HermesChatMessage::Open)).await;
    assert!(!app.hermes_chat_ctx().ai_on);
    let turn_on = chat_chip(&app, &ncrs_hermes_chat::ChipReply::TurnOnAi);

    settle(&mut app, turn_on).await;

    assert!(
        config_text(&config_dir).contains("enabled = true"),
        "{}",
        config_text(&config_dir)
    );
    assert_ne!(app.hermes.filing_config.ai, AiSetting::Off);
    assert!(app.hermes_chat_ctx().ai_on);
    assert!(app
        .hermes_chat
        .thread()
        .iter()
        .any(|entry| matches!(entry, ncrs_hermes_chat::Entry::User(_))));
}

#[tokio::test]
async fn loading_the_model_with_the_ai_off_also_switches_the_ai_on() {
    let config_dir = tempfile::tempdir().unwrap();
    let mut app = chat_with_ai_off(&config_dir);
    app.hermes.index_path = Some(config_dir.path().join("index.db"));

    let task = app.run_hermes_chat_action(ncrs_hermes_chat::Action::DownloadModel);
    settle_task(&mut app, task).await;

    assert!(
        config_text(&config_dir).contains("enabled = true"),
        "{}",
        config_text(&config_dir)
    );
    assert_ne!(app.hermes.filing_config.ai, AiSetting::Off);
}

#[test]
fn with_the_ai_off_the_chat_gets_no_model() {
    let mut app = App::with_fixed_panels();
    app.chat_model = Some(Arc::new(ScriptedChatModel::new(
        LocalModelStatus::NotDownloaded,
        [],
    )));
    assert_eq!(app.hermes.filing_config.ai, AiSetting::Off);
    let ctx_off = app.hermes_chat_ctx();
    assert!(!ctx_off.ai_on);
    assert!(ctx_off.model.is_none());

    app.hermes.filing_config.ai = AiSetting::Local { threads: None };
    let ctx_on = app.hermes_chat_ctx();
    assert!(ctx_on.ai_on);
    assert!(ctx_on.model.is_some());
}

#[tokio::test]
async fn loading_the_model_from_the_chat_opens_the_card_and_runs_the_download() {
    let config_dir = tempfile::tempdir().unwrap();
    let mut app = chat_with_ai_off(&config_dir);
    app.hermes.filing_config.ai = AiSetting::Local { threads: None };
    app.hermes.index_path = Some(config_dir.path().join("index.db"));
    app.download_ai_model = quiet_download;

    drop(app.run_hermes_chat_action(ncrs_hermes_chat::Action::DownloadModel));

    assert!(app.ai_model_for_test().is_some());
    assert!(app.settings.ai_model_download_runs());
}

#[tokio::test]
async fn a_chat_opened_while_hermes_starts_greets_at_once_and_only_once() {
    let mut app = App::with_fixed_panels()
        .in_language(Language::German)
        .with_starting_hermes_for_test();
    settle(&mut app, Message::HermesChat(HermesChatMessage::Open)).await;
    assert!(matches!(
        app.hermes_chat_ctx().availability,
        ncrs_hermes_chat::Availability::Starting
    ));
    assert_eq!(app.hermes_chat.thread().len(), 1);
    assert!(!app.hermes_chat.chips().is_empty());

    app.hermes.filing = Some(FilingModule::in_phase(Phase::Ready));
    settle(&mut app, Message::NoticeExpired(0)).await;

    assert_eq!(app.hermes_chat.thread().len(), 1);
    assert!(!app.hermes_chat.waiting_for_hermes);
}

#[tokio::test]
async fn a_chat_waiting_for_hermes_says_it_needs_setup_when_the_start_fails() {
    let mut app = App::with_fixed_panels()
        .in_language(Language::German)
        .with_starting_hermes_for_test();
    settle(&mut app, Message::HermesChat(HermesChatMessage::Open)).await;
    assert!(app.hermes_chat.waiting_for_hermes);

    app.hermes.filing = Some(FilingModule::in_phase(Phase::Failed("no key".into())));
    settle(&mut app, Message::NoticeExpired(0)).await;

    assert!(!app.hermes_chat.waiting_for_hermes);
    let Some(ncrs_hermes_chat::Entry::Hermes { parts, .. }) = app.hermes_chat.thread().last()
    else {
        panic!("the thread ends without a Hermes entry");
    };
    assert_eq!(
        parts,
        &[ncrs_hermes_chat::Part::Text(
            Language::German
                .text(ncrs_i18n::Msg::HermesChatOff)
                .to_owned()
        )]
    );
}
