use iced_test::Simulator;

use crate::app::*;
use crate::filing::AiStatus;
use crate::i18n::Language;

fn simulator(app: &App) -> Simulator<'_, Message, iced::Theme> {
    Simulator::with_size(
        iced::Settings {
            default_font: ncrs_ui_kit::APP_FONT,
            ..iced::Settings::default()
        },
        iced::Size::new(1200.0, 760.0),
        app.view(),
    )
}

fn shows(ai: AiStatus, label: &str) -> bool {
    simulator(&App::with_filing_status_ai(ai))
        .find(label)
        .is_ok()
}

#[test]
fn the_row_is_not_there_while_the_ai_is_off() {
    assert!(!shows(AiStatus::Off, "AI model"));
}

#[test]
fn the_row_names_the_state_of_the_model() {
    assert!(shows(AiStatus::NotDownloaded, "AI model"));
    assert!(shows(AiStatus::NotDownloaded, "not downloaded"));
    assert!(shows(AiStatus::NotLoadedYet, "not loaded yet"));
    assert!(shows(
        AiStatus::Failed("the file is damaged"),
        "failed: the file is damaged"
    ));
    assert!(shows(
        AiStatus::Http {
            origin: "http://localhost:8080".to_owned(),
            model: "qwen".to_owned()
        },
        "qwen at http://localhost:8080"
    ));
}

fn loaded_on(compute: ncrs_ai::Compute) -> AiStatus {
    AiStatus::Loaded {
        name: "Qwen3-4B-Q4_K_M.gguf".to_owned(),
        memory_bytes: 2_700_000_000,
        compute,
    }
}

#[test]
fn the_loaded_row_says_whether_the_model_computes_on_a_graphics_card_or_the_processor() {
    let line = |computes_on: &str| {
        format!(
            "Qwen3-4B-Q4_K_M.gguf · {} in memory · runs on {computes_on}",
            crate::ui::format::size(2_700_000_000)
        )
    };
    let on_gpu = loaded_on(ncrs_ai::Compute::Gpu("Apple M2 Pro".to_owned()));
    assert!(shows(on_gpu, &line("Apple M2 Pro")));
    assert!(shows(loaded_on(ncrs_ai::Compute::Cpu), &line("CPU")));
}

#[test]
fn the_loaded_row_matches_its_snapshots() {
    for (status, name) in [
        (
            loaded_on(ncrs_ai::Compute::Gpu("Apple M2 Pro".to_owned())),
            "filing_status_ai_gpu_de",
        ),
        (loaded_on(ncrs_ai::Compute::Cpu), "filing_status_ai_cpu_de"),
    ] {
        let app = App::with_filing_status_ai(status).in_language(Language::German);
        let matches = simulator(&app)
            .snapshot(&iced::Theme::Dark)
            .unwrap()
            .matches_image(format!("tests/snapshots/{name}.png"))
            .expect("snapshot comparison");
        assert!(
            matches,
            "the page does not match tests/snapshots/{name}.png"
        );
    }
}
