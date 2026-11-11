#![allow(clippy::panic_in_result_fn)]

use std::error::Error;

use iced_test::Simulator;
use ncrs_settings::facts::{AreaFacts, LoadedFacts, RunDay, RunFacts};

use super::settings_pages_tests::{app_with_file, settings, settle, written, TestResult};
use crate::app::*;
use crate::config::from_text;
use crate::i18n::Language;
use crate::messages::SettingsFeature;

fn filled_config() -> &'static str {
    "[modules.filing]\nenabled = true\nown_names = [\"Max Muster\", \"Muster GmbH\"]\nown_ibans = [\"DE89370400440532013000\"]\nown_tax_ids = [\"12/345/67890\"]\n\n[[modules.filing.locations]]\npath = \"/home/test/Downloads/Scans\"\nmode = \"inbox\"\nimages = true\n\n[[modules.filing.locations]]\npath = \"/home/test/Documents/Filed\"\nmode = \"archive\"\ntarget = true\n"
}

fn loaded_areas() -> LoadedFacts {
    let area = |name: &str, files: usize, order: &str| AreaFacts {
        name: name.to_owned(),
        files,
        read_folder: PathBuf::from("/home/test/Downloads/Scans"),
        root: PathBuf::from("/home/test/Downloads/Scans").join(name),
        order: order.to_owned(),
    };
    let run = |batch: i64,
               day: RunDay,
               time: &str,
               files: usize,
               to: &str,
               examples: &[&str],
               undoable: bool| RunFacts {
        batch,
        day,
        time: time.to_owned(),
        files,
        from: "Downloads".to_owned(),
        to: to.to_owned(),
        examples: examples
            .iter()
            .map(|example| (*example).to_owned())
            .collect(),
        undoable,
    };
    LoadedFacts {
        areas: vec![
            area("Steuern", 41, "nach Jahr"),
            area("Versicherung", 17, "nach Anbieter"),
            area("Gesundheit", 9, "nach Monat"),
            area("Auto", 4, ""),
        ],
        runs: vec![
            run(
                4,
                RunDay::Today,
                "14:02",
                12,
                "Dokumente",
                &["Steuern/2025", "Versicherung/Allianz"],
                true,
            ),
            run(
                3,
                RunDay::Yesterday,
                "09:30",
                1,
                "Dokumente",
                &["Auto"],
                false,
            ),
            run(
                2,
                RunDay::On("2026-10-30".to_owned()),
                "18:45",
                7,
                "",
                &[],
                true,
            ),
            run(
                1,
                RunDay::On("2026-10-28".to_owned()),
                "08:15",
                3,
                "Dokumente",
                &[],
                true,
            ),
        ],
        folder_files: std::collections::BTreeMap::from([(
            PathBuf::from("/home/test/Downloads/Scans"),
            214,
        )]),
        understood: None,
        disconnected: vec![PathBuf::from("/home/test/Documents/Filed")],
    }
}

fn page(
    feature: SettingsFeature,
    source: &str,
    loaded: Option<LoadedFacts>,
) -> Result<App, Box<dyn Error>> {
    let mut app = App::with_settings_file(feature, Ok(from_text(source)?));
    if loaded.as_ref().is_some_and(|facts| !facts.areas.is_empty()) {
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
        .matches_image(format!("tests/snapshots/{name}.png"))?;
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
        Some(loaded_areas()),
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

#[test]
fn the_hermes_page_matches_its_snapshots_filled() -> TestResult {
    in_both_languages(
        SettingsFeature::Hermes,
        filled_config(),
        Some(loaded_areas()),
        "settings_hermes_filled",
        WHOLE_PAGE_HEIGHT,
    )
}

#[test]
fn the_hermes_page_matches_its_snapshots_empty() -> TestResult {
    in_both_languages(
        SettingsFeature::Hermes,
        "",
        Some(LoadedFacts::default()),
        "settings_hermes_empty",
        WINDOW_HEIGHT,
    )
}

#[tokio::test]
async fn zeigen_on_the_unreadable_files_opens_the_inspection_page() -> TestResult {
    let mut app = page(
        SettingsFeature::Index,
        filled_config(),
        Some(loaded_areas()),
    )?;
    let mut simulator = page_simulator(&app);
    simulator.click("Show")?;
    let messages: Vec<Message> = simulator.into_messages().collect();
    let show = settings(SettingsMessage::Select(SettingsFeature::ReadErrors));
    let focused = settings(SettingsMessage::FocusThen(
        11,
        Some(Box::new(SettingsMessage::Select(
            SettingsFeature::ReadErrors,
        ))),
    ));
    assert_eq!(messages, std::slice::from_ref(&focused));

    drop(app.update(show));
    assert!(app.inspection_page_is_open_for_test());
    Ok(())
}

#[test]
fn the_folder_dialog_matches_its_snapshots() -> TestResult {
    for (language, suffix) in [(Language::German, "de"), (Language::English, "en")] {
        let mut app = page(
            SettingsFeature::Hermes,
            filled_config(),
            Some(loaded_areas()),
        )?
        .in_language(language);
        if let Some(state) = app.settings.page.as_mut() {
            state.folder_dialog = Some(PathBuf::from("/home/test/Downloads/Scans"));
        }
        matches_snapshot(
            &app,
            &format!("settings_hermes_folder_dialog_{suffix}"),
            WINDOW_HEIGHT,
        )?;
    }
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

#[test]
fn only_a_run_that_can_be_put_back_has_the_undo_button() -> TestResult {
    let mut loaded = loaded_areas();
    loaded.runs.first_mut().ok_or("no run")?.undoable = false;
    let app = page(SettingsFeature::Hermes, filled_config(), Some(loaded))?
        .in_language(Language::English);
    let mut simulator = page_simulator(&app);
    simulator.click("Undo")?;
    let messages: Vec<Message> = simulator.into_messages().collect();
    assert_eq!(
        messages,
        vec![settings(SettingsMessage::FocusThen(
            6,
            Some(Box::new(SettingsMessage::UndoRun(2)))
        ))]
    );
    Ok(())
}

#[test]
fn show_all_lists_the_runs_beyond_the_last_three() -> TestResult {
    let mut app = page(
        SettingsFeature::Hermes,
        filled_config(),
        Some(loaded_areas()),
    )?
    .in_language(Language::English);
    let fourth = "2026-10-28 08:15 · 3 files from Downloads to Dokumente";
    let shown_before = page_simulator(&app).find(fourth).is_ok();
    assert!(!shown_before);

    drop(app.update(settings(SettingsMessage::ToggleAllRuns)));

    let shown_after = page_simulator(&app).find(fourth).is_ok();
    assert!(shown_after);
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
        Some(loaded_areas()),
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
        Some(loaded_areas()),
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
        Some(loaded_areas()),
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
        Some(loaded_areas()),
    )?;
    let asked_long_ago = long_ago();
    app.facts_requested_at = Some(asked_long_ago);
    drop(app.refresh_documents_facts());
    assert_eq!(app.facts_requested_at, Some(asked_long_ago));
    Ok(())
}
