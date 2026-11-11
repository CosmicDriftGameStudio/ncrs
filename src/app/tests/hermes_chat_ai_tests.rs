//! The chat's AI offers write the same setting as the overview checkbox.

use crate::app::*;
use iced_test::futures::futures::StreamExt as _;
use iced_test::runtime::{task, Action};
use ncrs_ai::{LocalModelStatus, ScriptedChatModel};

use crate::config::AiSetting;
use ncrs_ops::NamesModel;

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
async fn a_chat_waiting_for_hermes_says_why_when_the_start_fails() {
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
            "Hermes konnte nicht starten: no key".to_owned()
        )]
    );
}

fn last_hermes_text(app: &App) -> String {
    let Some(ncrs_hermes_chat::Entry::Hermes { parts, .. }) = app.hermes_chat.thread().last()
    else {
        panic!("the thread ends without a Hermes entry");
    };
    match parts.as_slice() {
        [ncrs_hermes_chat::Part::Text(text)] => text.clone(),
        other => panic!("not one sentence: {other:?}"),
    }
}

fn user_entries(app: &App) -> usize {
    app.hermes_chat
        .thread()
        .iter()
        .filter(|entry| matches!(entry, ncrs_hermes_chat::Entry::UserFolder(_)))
        .count()
}

#[tokio::test]
async fn a_module_that_failed_to_start_says_why_and_offers_to_try_again() {
    let mut app = App::with_fixed_panels().in_language(Language::German);
    app.hermes.filing = Some(FilingModule::in_phase(Phase::Failed("no key".into())));
    assert!(matches!(
        app.hermes_chat_ctx().availability,
        ncrs_hermes_chat::Availability::Failed(_)
    ));

    settle(&mut app, Message::HermesChat(HermesChatMessage::Open)).await;

    assert_eq!(
        last_hermes_text(&app),
        "Hermes konnte nicht starten: no key"
    );
    let retry = app
        .hermes_chat
        .chips()
        .iter()
        .find(|chip| chip.reply == ncrs_hermes_chat::ChipReply::TurnOn)
        .map(|chip| chip.label.clone());
    assert_eq!(retry.as_deref(), Some("Nochmal versuchen"));
}

#[tokio::test]
async fn a_config_write_that_fails_on_turning_hermes_on_is_said_in_the_chat() {
    let config_dir = tempfile::tempdir().unwrap();
    let mut app = App::with_fixed_panels().in_language(Language::German);
    app.hermes.filing_config.first_run = false;
    app.config_path = Some(config_dir.path().to_path_buf());
    settle(&mut app, Message::HermesChat(HermesChatMessage::Open)).await;
    let turn_on = chat_chip(&app, &ncrs_hermes_chat::ChipReply::TurnOn);

    settle(&mut app, turn_on).await;

    assert!(last_hermes_text(&app).starts_with("Ich konnte Hermes nicht einschalten: "));
    assert!(!app.turning_hermes_on);
    assert!(matches!(
        app.hermes_chat_ctx().availability,
        ncrs_hermes_chat::Availability::Off
    ));
    chat_chip(&app, &ncrs_hermes_chat::ChipReply::TurnOn);
}

static GRANTED: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

fn recording_grant(_: &Path, folder: &Path) -> Result<(), String> {
    GRANTED
        .lock()
        .map_err(|err| err.to_string())?
        .push(folder.to_path_buf());
    Ok(())
}

fn refusing_grant(_: &Path, _: &Path) -> Result<(), String> {
    Err("bookmark refused".to_owned())
}

fn sandboxed_chat(grant: fn(&Path, &Path) -> Result<(), String>) -> App {
    let mut app = App::with_fixed_panels().in_language(Language::German);
    app.hermes.filing_config.first_run = false;
    app.sandbox = fs::SandboxAccess {
        active: true,
        grants_dir: Some(PathBuf::from("grants-of-this-test")),
        home_readable: || true,
        grant,
        covers: |_, _| true,
    };
    app
}

#[tokio::test]
async fn a_folder_picked_in_the_chat_is_granted_before_the_chat_reads_it() {
    let mut app = sandboxed_chat(recording_grant);
    settle(&mut app, Message::HermesChat(HermesChatMessage::Open)).await;
    let folder = PathBuf::from("/chat-grant-test/Invoices");

    let task = app.update(Message::HermesChat(HermesChatMessage::FolderPicked(Some(
        folder.clone(),
    ))));
    assert_eq!(user_entries(&app), 0, "nothing is read before the grant");
    settle_task(&mut app, task).await;

    assert!(GRANTED.lock().unwrap().contains(&folder));
    assert_eq!(user_entries(&app), 1);
}

#[tokio::test]
async fn a_refused_grant_for_a_picked_folder_is_said_in_the_chat() {
    let mut app = sandboxed_chat(refusing_grant);
    settle(&mut app, Message::HermesChat(HermesChatMessage::Open)).await;

    settle(
        &mut app,
        Message::HermesChat(HermesChatMessage::FolderPicked(Some(PathBuf::from(
            "/chat-grant-test/Refused",
        )))),
    )
    .await;

    assert_eq!(
        last_hermes_text(&app),
        "Das konnte ich nicht lesen: bookmark refused"
    );
    assert_eq!(user_entries(&app), 0);
    assert!(!app.hermes_chat.chips().is_empty());
}

#[test]
fn the_batch_rename_tells_a_model_on_this_computer_from_one_elsewhere() {
    let mut app = App::with_fixed_panels();
    app.chat_model = Some(Arc::new(ScriptedChatModel::new(
        LocalModelStatus::NotDownloaded,
        [],
    )));
    let setting = |endpoint: &str| AiSetting::Http {
        endpoint: endpoint.to_owned(),
        model: "m".to_owned(),
    };

    app.hermes.filing_config.ai = AiSetting::Local { threads: None };
    assert!(matches!(app.chat_model_for_names(), NamesModel::Local(_)));
    app.hermes.filing_config.ai = setting("http://localhost:11434/v1");
    assert!(matches!(app.chat_model_for_names(), NamesModel::Local(_)));
    app.hermes.filing_config.ai = setting("https://models.example.org/v1");
    assert!(matches!(app.chat_model_for_names(), NamesModel::Remote(_)));
    app.hermes.filing_config.ai = AiSetting::Off;
    assert!(matches!(app.chat_model_for_names(), NamesModel::None));
}
