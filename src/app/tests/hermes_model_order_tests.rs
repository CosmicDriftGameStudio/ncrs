//! The local model and an own server on localhost, set up in either order,
//! leave Hermes working with the server.

use crate::app::*;
use crate::config::AiSetting;
use crate::messages::{SettingChange, SettingField, SettingsFeature};
use iced_test::futures::futures::StreamExt as _;
use iced_test::runtime::{task, Action};

fn quiet_download(_: &DownloadJob) -> Result<(), AiDownloadError> {
    Ok(())
}

async fn settle(app: &mut App, message: Message) {
    let mut queue = vec![app.update(message)];
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

fn app_on_the_index_page(config_dir: &tempfile::TempDir) -> App {
    let source = "[modules.filing]\nenabled = true\n";
    let config_path = config_dir.path().join("config.toml");
    std::fs::write(&config_path, source).unwrap();
    let config = crate::config::from_text(source).unwrap();
    let mut app = App::with_settings_file(SettingsFeature::Index, Ok(config));
    app.config_path = Some(config_path);
    app.hermes.index_path = Some(config_dir.path().join("index.db"));
    app.download_ai_model = quiet_download;
    app
}

async fn load_the_local_model(app: &mut App) {
    settle(app, Message::Settings(SettingsMessage::OpenAiModel)).await;
    settle(app, Message::AiModel(AiModelMessage::Download)).await;
}

async fn set_up_the_server_on_localhost(app: &mut App) {
    settle(app, Message::Settings(SettingsMessage::ServerDialog(true))).await;
    settle(
        app,
        Message::Settings(SettingsMessage::ChooseProvider(
            crate::config::AiProviderName::Http,
        )),
    )
    .await;
    for (field, text) in [
        (SettingField::AiEndpoint, "http://localhost:11434"),
        (SettingField::AiModel, "qwen2.5:7b"),
    ] {
        settle(
            app,
            Message::Settings(SettingsMessage::Edit {
                field,
                text: text.to_owned(),
            }),
        )
        .await;
    }
    settle(app, Message::Settings(SettingsMessage::SaveServer)).await;
}

fn assert_hermes_uses_the_server(app: &mut App) {
    assert!(
        matches!(
            &app.hermes.filing_config.ai,
            AiSetting::Http { endpoint, .. } if endpoint == "http://localhost:11434"
        ),
        "{:?}",
        app.hermes.filing_config.ai
    );
    assert_eq!(
        models_asked(app),
        (true, true),
        "the chat and the rename get the server"
    );
}

#[tokio::test]
async fn a_server_set_up_after_the_download_is_what_hermes_uses() {
    let config_dir = tempfile::tempdir().unwrap();
    let mut app = app_on_the_index_page(&config_dir);

    load_the_local_model(&mut app).await;
    set_up_the_server_on_localhost(&mut app).await;

    assert_hermes_uses_the_server(&mut app);
}

#[tokio::test]
async fn a_server_set_up_during_the_download_is_what_hermes_uses() {
    let config_dir = tempfile::tempdir().unwrap();
    let mut app = app_on_the_index_page(&config_dir);

    settle(&mut app, Message::Settings(SettingsMessage::OpenAiModel)).await;
    drop(app.update(Message::AiModel(AiModelMessage::Download)));
    let generation = app.settings.ai_model_generation;
    set_up_the_server_on_localhost(&mut app).await;
    settle(
        &mut app,
        Message::AiModel(AiModelMessage::Downloaded {
            generation,
            result: Ok(()),
        }),
    )
    .await;

    assert_hermes_uses_the_server(&mut app);
}

#[tokio::test]
async fn a_server_alone_is_what_hermes_uses() {
    let config_dir = tempfile::tempdir().unwrap();
    let mut app = app_on_the_index_page(&config_dir);

    set_up_the_server_on_localhost(&mut app).await;

    assert_hermes_uses_the_server(&mut app);
}

#[tokio::test]
async fn a_stopped_download_then_a_server_is_what_hermes_uses() {
    let config_dir = tempfile::tempdir().unwrap();
    let mut app = app_on_the_index_page(&config_dir);

    settle(&mut app, Message::Settings(SettingsMessage::OpenAiModel)).await;
    drop(app.update(Message::AiModel(AiModelMessage::Download)));
    settle(&mut app, Message::AiModel(AiModelMessage::Stop)).await;
    set_up_the_server_on_localhost(&mut app).await;

    assert_hermes_uses_the_server(&mut app);
}

fn config_text(config_dir: &tempfile::TempDir) -> String {
    std::fs::read_to_string(config_dir.path().join("config.toml")).unwrap()
}

async fn switch_hermes(app: &mut App, on: bool) {
    settle(
        app,
        Message::Settings(SettingsMessage::Change(SettingChange::HermesSwitch(on))),
    )
    .await;
}

/// What the chat and the batch rename are given to ask.
fn models_asked(app: &mut App) -> (bool, bool) {
    app.ensure_chat_model();
    let chat = app.hermes_chat_ctx();
    (chat.ai_on, chat.model.is_some())
}

#[tokio::test]
async fn switched_off_no_model_is_given_to_anything_and_the_file_says_so() {
    let config_dir = tempfile::tempdir().unwrap();
    let mut app = app_on_the_index_page(&config_dir);
    set_up_the_server_on_localhost(&mut app).await;
    assert_eq!(models_asked(&mut app), (true, true));

    switch_hermes(&mut app, false).await;

    assert_eq!(app.hermes.filing_config.ai, AiSetting::Off);
    assert!(
        !app.hermes.filing_config.enabled,
        "no read folder is watched"
    );
    assert_eq!(models_asked(&mut app), (false, false));
    assert!(config_text(&config_dir).contains("[hermes]"));
    assert!(config_text(&config_dir).contains("enabled = false"));
}

#[tokio::test]
async fn switched_on_again_the_server_is_used_without_setting_it_up_again() {
    let config_dir = tempfile::tempdir().unwrap();
    let mut app = app_on_the_index_page(&config_dir);
    set_up_the_server_on_localhost(&mut app).await;
    switch_hermes(&mut app, false).await;

    switch_hermes(&mut app, true).await;

    assert_hermes_uses_the_server(&mut app);
    assert!(
        app.hermes.filing_config.enabled,
        "the folders are watched again"
    );
    assert_eq!(models_asked(&mut app), (true, true));
}

#[tokio::test]
async fn a_download_does_not_flip_the_switch() {
    let config_dir = tempfile::tempdir().unwrap();
    let mut app = app_on_the_index_page(&config_dir);
    switch_hermes(&mut app, false).await;

    load_the_local_model(&mut app).await;

    assert_eq!(app.hermes.filing_config.ai, AiSetting::Off);
    assert!(config_text(&config_dir).contains("[hermes]\nenabled = false"));
}

/// A server on 127.0.0.1 that answers one request with a minimal
/// OpenAI-compatible reply and tells whether a request came in.
fn stub_server() -> (u16, std::thread::JoinHandle<bool>) {
    use std::io::{BufRead as _, BufReader, Read as _, Write as _};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut length = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = value.trim().parse().unwrap();
            }
            if line == "\r\n" {
                break;
            }
        }
        reader.read_exact(&mut vec![0; length]).unwrap();
        let body = "{\"choices\": [{\"message\": {\"content\": \"{}\"}}]}";
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(reply.as_bytes()).unwrap();
        true
    });
    (port, handle)
}

#[tokio::test]
async fn the_reported_bug_download_then_a_server_on_127_0_0_1_gets_the_chat_call() {
    let (port, stub) = stub_server();
    let config_dir = tempfile::tempdir().unwrap();
    let mut app = app_on_the_index_page(&config_dir);
    load_the_local_model(&mut app).await;
    settle(
        &mut app,
        Message::Settings(SettingsMessage::ServerDialog(true)),
    )
    .await;
    settle(
        &mut app,
        Message::Settings(SettingsMessage::ChooseProvider(
            crate::config::AiProviderName::Http,
        )),
    )
    .await;
    for (field, text) in [
        (
            SettingField::AiEndpoint,
            format!("http://127.0.0.1:{port}/v1"),
        ),
        (SettingField::AiModel, "m".to_owned()),
    ] {
        settle(
            &mut app,
            Message::Settings(SettingsMessage::Edit { field, text }),
        )
        .await;
    }
    settle(&mut app, Message::Settings(SettingsMessage::SaveServer)).await;

    assert!(app.hermes.filing_config.hermes_on());
    assert!(matches!(
        app.hermes.filing_config.ai,
        AiSetting::Http { .. }
    ));
    assert!(!config_text(&config_dir).contains("[modules.filing.ai]\nenabled"));
    app.ensure_chat_model();
    let model = app.chat_model.clone().expect("the chat has the server");
    let turn = ncrs_ai::ChatTurn {
        message: "hello".to_owned(),
        question: None,
        previous_search: None,
        previous_request: None,
        previous_message: None,
    };
    drop(model.understand(&turn));
    assert!(stub.join().unwrap(), "the request reached the server");
}

#[tokio::test]
async fn the_switch_off_is_in_the_file_and_leaves_no_chat_model_and_on_brings_it_back() {
    let config_dir = tempfile::tempdir().unwrap();
    let mut app = app_on_the_index_page(&config_dir);
    set_up_the_server_on_localhost(&mut app).await;
    assert!(models_asked(&mut app).1);

    switch_hermes(&mut app, false).await;

    assert!(config_text(&config_dir).contains("[hermes]\nenabled = false"));
    assert_eq!(app.hermes.filing_config.ai, AiSetting::Off);
    app.ensure_chat_model();
    assert!(app.chat_model.is_none());
    assert_eq!(models_asked(&mut app), (false, false));

    switch_hermes(&mut app, true).await;

    assert_hermes_uses_the_server(&mut app);
    assert!(app.chat_model.is_some());
}

fn the_index_ai_state(app: &App) -> ncrs_settings::facts::AiState {
    let state = app.settings.page.as_ref().expect("a settings page");
    app.settings_view(state).facts.ai
}

#[tokio::test]
async fn an_unconsented_remote_server_is_not_allowed_and_saving_it_unchanged_asks_once() {
    let source = "[modules.filing.ai]\nenabled = false\nprovider = \"http\"\n\
                  endpoint = \"https://unreachable.invalid\"\nmodel = \"m\"\n";
    let config_dir = tempfile::tempdir().unwrap();
    let config_path = config_dir.path().join("config.toml");
    std::fs::write(&config_path, source).unwrap();
    let config = crate::config::from_text(source).unwrap();
    let mut app = App::with_settings_file(SettingsFeature::Index, Ok(config));
    app.config_path = Some(config_path);
    app.hermes.index_path = Some(config_dir.path().join("index.db"));
    assert_eq!(
        the_index_ai_state(&app),
        ncrs_settings::facts::AiState::NotAllowed
    );

    settle(
        &mut app,
        Message::Settings(SettingsMessage::ServerDialog(true)),
    )
    .await;
    settle(&mut app, Message::Settings(SettingsMessage::SaveServer)).await;
    let asked = app
        .settings
        .page
        .as_ref()
        .and_then(|state| state.consent_pending.as_ref())
        .map(|ask| ask.origin.clone());
    assert_eq!(asked.as_deref(), Some("https://unreachable.invalid"));
    assert!(!config_text(&config_dir).contains("consented_origins"));

    settle(
        &mut app,
        Message::Settings(SettingsMessage::ConfirmAiConsent),
    )
    .await;

    assert!(
        config_text(&config_dir).contains("consented_origins = [\"https://unreachable.invalid\"]")
    );
    assert_ne!(
        the_index_ai_state(&app),
        ncrs_settings::facts::AiState::NotAllowed
    );
}
