#![allow(
    clippy::panic_in_result_fn,
    reason = "a failing assertion is the signal in a test"
)]

use ncrs_ui_kit::snapshot_support::ReferenceSnapshot as _;
use std::error::Error;

use iced_test::Simulator;
use ncrs_settings::facts::LoadedFacts;

use super::settings_pages_tests::{app_with_file, settings, settle, written, TestResult};
use crate::app::*;
use crate::config::from_text;
use crate::i18n::Language;
use crate::messages::SettingsFeature;

// Windows does not count a path without a drive as absolute, and the config
// refuses a relative location; the macOS pictures keep the Unix path.
#[cfg(windows)]
macro_rules! home {
    () => {
        "C:/home/test"
    };
}
#[cfg(not(windows))]
macro_rules! home {
    () => {
        "/home/test"
    };
}

fn filled_config() -> &'static str {
    concat!(
        "[modules.filing]\nenabled = true\nown_names = [\"Max Muster\", \"Muster GmbH\"]\nown_ibans = [\"DE89370400440532013000\"]\nown_tax_ids = [\"12/345/67890\"]\n\n[[modules.filing.locations]]\npath = \"",
        home!(),
        "/Downloads/Scans\"\nmode = \"inbox\"\nimages = true\n\n[[modules.filing.locations]]\npath = \"",
        home!(),
        "/Documents/Filed\"\nmode = \"archive\"\ntarget = true\n"
    )
}

fn loaded_facts() -> LoadedFacts {
    LoadedFacts {
        folder_files: std::collections::BTreeMap::from([(
            PathBuf::from(concat!(home!(), "/Downloads/Scans")),
            214,
        )]),
        read_folders: Some(ncrs_settings::facts::ReadFolderCoverage {
            documents: 312,
            with_text: 300,
            understood: 298,
        }),
        disconnected: vec![PathBuf::from(concat!(home!(), "/Documents/Filed"))],
        error: None,
    }
}

fn page(
    feature: SettingsFeature,
    source: &str,
    loaded: Option<LoadedFacts>,
) -> Result<App, Box<dyn Error>> {
    let mut app = App::with_settings_file(feature, Ok(from_text(source)?));
    if loaded.as_ref().is_some_and(|facts| !facts.folder_files.is_empty()) {
        app = app.with_ready_index_for_test();
    }
    if let Some(state) = app.settings.page.as_mut() {
        state.loaded = loaded;
        state.facts_requested = true;
    }
    Ok(app)
}

/// A window tall enough for a whole page, so that a snapshot shows every row.
const WHOLE_PAGE_HEIGHT: f32 = 1100.0;
const WINDOW_HEIGHT: f32 = 760.0;

fn matches_snapshot(app: &App, name: &str, height: f32) -> TestResult {
    let mut simulator = Simulator::with_size(
        iced::Settings {
            default_font: ncrs_ui_kit::APP_FONT,
            ..iced::Settings::default()
        },
        iced::Size::new(1200.0, height),
        app.view(),
    );
    let matches = simulator
        .snapshot(&iced::Theme::Dark)?
        .matches_reference_image(format!("tests/snapshots/{name}.png"))?;
    assert!(
        matches,
        "the page does not match tests/snapshots/{name}.png"
    );
    Ok(())
}

fn in_both_languages(
    feature: SettingsFeature,
    source: &str,
    loaded: Option<LoadedFacts>,
    name: &str,
    height: f32,
) -> TestResult {
    for (language, suffix) in [(Language::German, "de"), (Language::English, "en")] {
        let app = page(feature, source, loaded.clone())?.in_language(language);
        matches_snapshot(&app, &format!("{name}_{suffix}"), height)?;
    }
    Ok(())
}

#[test]
fn the_index_page_matches_its_snapshots_filled() -> TestResult {
    in_both_languages(
        SettingsFeature::Index,
        filled_config(),
        Some(loaded_facts()),
        "settings_index_filled",
        WHOLE_PAGE_HEIGHT,
    )
}

#[test]
fn the_index_page_matches_its_snapshots_empty() -> TestResult {
    in_both_languages(
        SettingsFeature::Index,
        "",
        None,
        "settings_index_empty",
        WINDOW_HEIGHT,
    )
}

#[tokio::test]
async fn zeigen_on_the_unreadable_files_opens_the_inspection_page() -> TestResult {
    let mut app = page(
        SettingsFeature::Index,
        filled_config(),
        Some(loaded_facts()),
    )?;
    let mut simulator = page_simulator(&app);
    simulator.click("Show")?;
    let messages: Vec<Message> = simulator.into_messages().collect();
    let show = settings(SettingsMessage::Select(SettingsFeature::ReadErrors));
    let focused = settings(SettingsMessage::FocusThen(
        10,
        Some(Box::new(SettingsMessage::Select(
            SettingsFeature::ReadErrors,
        ))),
    ));
    assert_eq!(messages, std::slice::from_ref(&focused));

    drop(app.update(show));
    assert!(app.inspection_page_is_open_for_test());
    Ok(())
}

fn page_simulator(app: &App) -> Simulator<'_, Message, iced::Theme> {
    Simulator::with_size(
        iced::Settings {
            default_font: ncrs_ui_kit::APP_FONT,
            ..iced::Settings::default()
        },
        iced::Size::new(1200.0, WHOLE_PAGE_HEIGHT),
        app.view(),
    )
}

/// From the right edge of a chip's label to the middle of its cross.
const CROSS_OFFSET: f32 = 12.0;

#[tokio::test]
async fn the_cross_on_an_own_name_takes_it_away_and_writes_the_file() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut app = app_with_file(SettingsFeature::Index, filled_config(), &dir)?;
    let mut simulator = page_simulator(&app);
    let chip = simulator.find("Max Muster")?.bounds();
    simulator.point_at(iced::Point::new(
        chip.x + chip.width + CROSS_OFFSET,
        chip.center_y(),
    ));
    let _ = simulator.simulate(iced_test::simulator::click());
    let messages: Vec<Message> = simulator.into_messages().collect();
    assert_eq!(messages.len(), 1, "{messages:?}");

    for message in messages {
        settle(&mut app, message).await;
    }

    let config = from_text(&written(&dir)?)?;
    assert_eq!(config.filing.own_names, ["Muster GmbH"]);
    assert_eq!(config.filing.own_ibans, ["DE89370400440532013000"]);
    Ok(())
}

fn long_ago() -> std::time::SystemTime {
    std::time::SystemTime::now() - std::time::Duration::from_secs(60)
}

#[test]
fn a_language_switch_reads_the_facts_of_the_open_page_again() -> TestResult {
    let mut app = page(
        SettingsFeature::Index,
        filled_config(),
        Some(loaded_facts()),
    )?;
    assert!(app.facts_requested_at.is_none());

    drop(app.update(Message::SwitchLanguage));

    assert!(app.facts_requested_at.is_some());
    Ok(())
}

#[test]
fn the_index_page_reads_its_facts_again_after_ten_seconds_and_not_before() -> TestResult {
    let mut app = page(
        SettingsFeature::Index,
        filled_config(),
        Some(loaded_facts()),
    )?;
    let recently = std::time::SystemTime::now() - std::time::Duration::from_secs(2);
    app.facts_requested_at = Some(recently);
    drop(app.refresh_documents_facts());
    assert_eq!(app.facts_requested_at, Some(recently));

    let asked_long_ago = long_ago();
    app.facts_requested_at = Some(asked_long_ago);
    drop(app.refresh_documents_facts());
    assert!(app.facts_requested_at > Some(asked_long_ago));
    Ok(())
}

#[test]
fn the_tick_does_not_read_the_facts_again_while_a_read_is_under_way() -> TestResult {
    let mut app = page(
        SettingsFeature::Index,
        filled_config(),
        Some(loaded_facts()),
    )?;
    let asked_long_ago = long_ago();
    app.facts_requested_at = Some(asked_long_ago);
    if let Some(state) = app.settings.page.as_mut() {
        state.facts_in_flight = true;
    }
    drop(app.refresh_documents_facts());
    assert_eq!(app.facts_requested_at, Some(asked_long_ago));
    Ok(())
}

#[test]
fn another_page_does_not_read_its_facts_on_the_tick() -> TestResult {
    let mut app = page(
        SettingsFeature::Hermes,
        filled_config(),
        Some(loaded_facts()),
    )?;
    let asked_long_ago = long_ago();
    app.facts_requested_at = Some(asked_long_ago);
    drop(app.refresh_documents_facts());
    assert_eq!(app.facts_requested_at, Some(asked_long_ago));
    Ok(())
}

#[test]
fn the_programs_page_with_type_rows_matches_its_snapshot() -> TestResult {
    let source = "[open]\nview = [\"less\"]\n\
        by_type = [\n\
        { extensions = [\"md\", \"markdown\"], with = \"internal\" },\n\
        { extensions = [\"pdf\"], with = \"system\" },\n\
        { extensions = [\"log\", \"txt\"], with = \"program\", program = \"code --goto {path}:{line}\" },\n\
        ]\n";
    let app = page(SettingsFeature::Programs, source, None)?;
    matches_snapshot(&app, "settings_programs_by_type", WHOLE_PAGE_HEIGHT)
}
