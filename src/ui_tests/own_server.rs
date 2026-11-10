//! Snapshots of the own AI server: its dialog and the Usage page.

// reason: a failing assertion is the signal in a test
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

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
