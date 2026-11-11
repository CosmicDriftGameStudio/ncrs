//! Snapshots of the own AI server: its dialog and the Usage page.

// reason: a failing assertion is the signal in a test
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use iced_test::Simulator;

use super::app_simulator as simulator;
use crate::app::App;
use crate::i18n::Language;
use crate::messages::{Message, SettingField, SettingsFeature, SettingsMessage};
use ncrs_settings::AiCheckResult;

fn matches_snapshot(app: &App, name: &str) {
    let matches = simulator(app)
        .snapshot(&iced::Theme::Dark)
        .unwrap()
        .matches_image(format!("tests/snapshots/{name}.png"))
        .expect("snapshot comparison");
    assert!(matches, "the tab does not match tests/snapshots/{name}.png");
}

fn both_languages(app: impl Fn() -> App, name: &str) {
    matches_snapshot(&app().in_language(Language::German), &format!("{name}_de"));
    matches_snapshot(&app().in_language(Language::English), &format!("{name}_en"));
}

fn settings_with(feature: SettingsFeature, source: &str) -> App {
    App::with_settings_file(feature, Ok(crate::config::from_text(source).unwrap()))
}

#[test]
fn an_own_ai_server_after_a_test_offers_its_models_in_a_dropdown() {
    both_languages(
        || {
            let mut app = settings_with(
                SettingsFeature::Hermes,
                "[modules.filing.ai]\nprovider = \"http\"\n\
                 endpoint = \"http://localhost:11434\"\nmodel = \"qwen2.5:7b\"\n\n\
                 [theme]\npreset = \"cosmic\"\n",
            );
            drop(app.update(Message::Settings(SettingsMessage::ServerDialog(true))));
            // The test starts a request task, which needs a runtime to be built; it is dropped unrun.
            let runtime = tokio::runtime::Runtime::new().unwrap();
            let _entered = runtime.enter();
            drop(app.update(Message::Settings(SettingsMessage::TestAiServer)));
            drop(app.update(Message::Settings(SettingsMessage::AiChecked {
                endpoint: "http://localhost:11434".to_owned(),
                result: AiCheckResult::Listed(vec![
                    "qwen2.5:7b".to_owned(),
                    "llama3.1:8b".to_owned(),
                    "mistral:7b".to_owned(),
                ]),
            })));
            app
        },
        "settings_ai_server_models",
    );
}

#[test]
fn an_own_ai_server_on_another_computer_matches_its_snapshots_with_the_warning() {
    both_languages(
        || {
            let mut app = settings_with(
                SettingsFeature::Hermes,
                "[modules.filing.ai]\nprovider = \"http\"\n\
                 endpoint = \"https://api.example.com/v1\"\nmodel = \"gpt-5\"\n\n\
                 [theme]\npreset = \"cosmic\"\n",
            );
            drop(app.update(Message::Settings(SettingsMessage::ServerDialog(true))));
            drop(app.update(Message::Settings(SettingsMessage::Edit {
                field: SettingField::AiEndpoint,
                text: "https://models.example.org/v1".to_owned(),
            })));
            app
        },
        "settings_ai_server_remote",
    );
}

fn usage_page_app() -> App {
    use chrono::TimeZone as _;
    let now = chrono::Local
        .with_ymd_and_hms(2026, 11, 9, 12, 0, 0)
        .unwrap();
    let record = |day: u32, model: &str, tokens_in: u64, cost: Option<f64>| {
        ncrs_ai::UsageRecord {
            time: chrono::Local
                .with_ymd_and_hms(2026, 11, day, 9, 0, 0)
                .unwrap()
                .with_timezone(&chrono::Utc),
            model: model.to_owned(),
            usage: ncrs_ai::Usage {
                tokens_in,
                tokens_out: tokens_in.div_euclid(10),
                cached: tokens_in.div_euclid(2),
                cost,
            },
        }
        .to_line()
    };
    let lines = [
        record(2, "gpt-5-mini", 41_000, Some(0.12)),
        record(3, "gpt-5-mini", 18_500, Some(0.05)),
        record(5, "gpt-5", 92_000, Some(1.4)),
        record(6, "gpt-5", 64_000, None),
        record(8, "qwen2.5:7b", 120_000, None),
        record(9, "gpt-5-mini", 30_000, Some(0.09)),
    ];
    let summaries = [7, 30, 90]
        .iter()
        .map(|&days| {
            (
                days,
                ncrs_ai::summarize(lines.iter().map(String::as_str), days, now),
            )
        })
        .collect();
    let mut app = settings_with(
        SettingsFeature::Usage,
        "[modules.filing.ai]\nprovider = \"http\"\n\
         endpoint = \"https://api.example.com/v1\"\nmodel = \"gpt-5\"\n\n\
         [theme]\npreset = \"cosmic\"\n",
    );
    drop(app.update(Message::Settings(SettingsMessage::UsageLoaded(summaries))));
    app
}

#[test]
fn the_usage_page_matches_its_snapshots() {
    both_languages(usage_page_app, "settings_usage");
}

#[test]
fn an_own_ai_server_that_answers_with_an_error_shows_its_status_in_the_dialog() {
    let mut app = settings_with(
        SettingsFeature::Hermes,
        "[modules.filing.ai]\nprovider = \"http\"\n\
         endpoint = \"http://localhost:11434\"\nmodel = \"qwen2.5:7b\"\n",
    )
    .in_language(Language::English);
    drop(app.update(Message::Settings(SettingsMessage::ServerDialog(true))));
    // The test starts a request task, which needs a runtime to be built; it is dropped unrun.
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _entered = runtime.enter();
    drop(app.update(Message::Settings(SettingsMessage::TestAiServer)));
    drop(app.update(Message::Settings(SettingsMessage::AiChecked {
        endpoint: "http://localhost:11434".to_owned(),
        result: AiCheckResult::ServerError(500),
    })));

    let shown = ncrs_i18n::fill_template(
        Language::English.text(ncrs_i18n::Msg::SettingsAiTestServerError),
        &[("{status}", "500")],
    );

    simulator(&app).find(shown.as_str()).unwrap();
}

/// Marc's window: the dialogs sit in the middle of this, not of the test size.
const MARCS_WINDOW: iced::Size = iced::Size::new(1740.0, 1130.0);

fn matches_snapshot_in_marcs_window(app: &mut App, name: &str) {
    drop(app.update(Message::WindowResized(MARCS_WINDOW)));
    let matches = Simulator::with_size(
        iced::Settings {
            default_font: ncrs_ui_kit::APP_FONT,
            ..iced::Settings::default()
        },
        MARCS_WINDOW,
        app.view(),
    )
    .snapshot(&iced::Theme::Dark)
    .unwrap()
    .matches_image(format!("tests/snapshots/{name}.png"))
    .expect("snapshot comparison");
    assert!(matches, "the tab does not match tests/snapshots/{name}.png");
}

fn from_this_computer(language: Language) -> App {
    settings_with(
        SettingsFeature::Hermes,
        "[modules.filing]\nenabled = true\n\n[modules.filing.ai]\nenabled = true\n\
         provider = \"local\"\n\n[theme]\npreset = \"cosmic\"\n",
    )
    .in_language(language)
}

#[test]
fn the_model_switch_on_the_index_page_matches_its_snapshots() {
    for (language, name) in [
        (Language::German, "settings_model_switch_de"),
        (Language::English, "settings_model_switch_en"),
    ] {
        let mut app = from_this_computer(language);
        drop(app.update(Message::Settings(SettingsMessage::Select(
            SettingsFeature::Index,
        ))));
        matches_snapshot_in_marcs_window(&mut app, name);
    }
}

#[test]
fn the_question_before_a_server_on_another_computer_matches_its_snapshots() {
    for (language, name) in [
        (Language::German, "settings_model_switch_ask_de"),
        (Language::English, "settings_model_switch_ask_en"),
    ] {
        let mut app = from_this_computer(language);
        drop(app.update(Message::Settings(SettingsMessage::Select(
            SettingsFeature::Index,
        ))));
        drop(app.update(Message::Settings(SettingsMessage::ServerDialog(true))));
        drop(
            app.update(Message::Settings(SettingsMessage::ChooseProvider(
                ncrs_config::config::AiProviderName::Http,
            ))),
        );
        for (field, text) in [
            (SettingField::AiEndpoint, "https://openrouter.ai/api/v1"),
            (SettingField::AiModel, "anthropic/claude-sonnet-4.5"),
        ] {
            drop(app.update(Message::Settings(SettingsMessage::Edit {
                field,
                text: text.to_owned(),
            })));
        }
        drop(app.update(Message::Settings(SettingsMessage::SaveServer)));
        matches_snapshot_in_marcs_window(&mut app, name);
    }
}

#[test]
fn the_hermes_page_with_the_model_switch_matches_its_snapshots() {
    for (language, name) in [
        (Language::German, "settings_hermes_switch_de"),
        (Language::English, "settings_hermes_switch_en"),
    ] {
        let mut app = from_this_computer(language);
        matches_snapshot_in_marcs_window(&mut app, name);
    }
}
