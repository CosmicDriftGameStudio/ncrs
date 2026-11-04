#![allow(clippy::panic_in_result_fn)]

use ncrs_settings::message::SettingField;
use std::error::Error;

use iced_test::futures::futures::StreamExt as _;
use iced_test::runtime::{task, Action};
use iced_test::Simulator;
use ncrs_settings::facts::{AreaFacts, LoadedFacts, RunDay, RunFacts};
use ncrs_settings::folders::AddAs;

use crate::app::*;
use crate::config::{from_text, OwnDetail};
use crate::i18n::Language;
use crate::messages::SettingsFeature;

type TestResult = Result<(), Box<dyn Error>>;

/// Delivers `message` and everything the tasks it starts report.
async fn settle(app: &mut App, message: Message) {
    let mut queue = vec![message];
    while let Some(next) = queue.pop() {
        let Some(mut stream) = task::into_stream(app.update(next)) else {
            continue;
        };
        while let Some(action) = stream.next().await {
            if let Action::Output(reported) = action {
                if !matches!(reported, Message::NoticeExpired(_)) {
                    queue.push(reported);
                }
            }
        }
    }
}

fn settings(change: SettingsMessage) -> Message {
    Message::Settings(change)
}

/// The settings open on `feature`, showing and writing the config file that
/// holds `source`.
fn app_with_file(
    feature: SettingsFeature,
    source: &str,
    dir: &tempfile::TempDir,
) -> Result<App, Box<dyn Error>> {
    let path = dir.path().join("config.toml");
    std::fs::write(&path, source)?;
    let mut app = App::with_settings_file(feature, Ok(from_text(source)?));
    app.config_path = Some(path);
    Ok(app)
}

fn written(dir: &tempfile::TempDir) -> Result<String, Box<dyn Error>> {
    Ok(std::fs::read_to_string(dir.path().join("config.toml"))?)
}

fn folder(dir: &tempfile::TempDir, name: &str) -> Result<std::path::PathBuf, Box<dyn Error>> {
    let path = dir.path().join(name);
    std::fs::create_dir(&path)?;
    Ok(std::fs::canonicalize(path)?)
}

fn locations_of(app: &App) -> Vec<crate::config::FilingLocation> {
    app.settings
        .page
        .as_ref()
        .and_then(|state| state.config())
        .map(|config| config.filing.locations.clone())
        .unwrap_or_default()
}

#[tokio::test]
async fn the_last_segment_of_the_index_page_writes_the_load_and_sets_it() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut app = app_with_file(SettingsFeature::Index, "", &dir)?;
    app.hermes.filing = Some(FilingModule::in_phase(Phase::Ready));
    app.hermes.pipeline = Some(ncrs_pipeline::Runner::start(
        ncrs_pipeline::Settings::new(Load::Gentle, std::num::NonZeroUsize::MIN.saturating_add(7)),
        Vec::new(),
        Vec::new(),
        std::sync::Arc::new(ncrs_pipeline::SystemClock),
    ));

    settle(
        &mut app,
        settings(SettingsMessage::Change(SettingChange::Load(Load::Full))),
    )
    .await;

    assert_eq!(app.hermes.filing_config.load, Load::Full);
    assert_eq!(
        app.hermes
            .pipeline
            .as_ref()
            .map(|runner| runner.snapshot().worker_target),
        Some(4)
    );
    assert!(written(&dir)?.contains("full"), "{}", written(&dir)?);
    Ok(())
}

#[tokio::test]
async fn a_folder_added_on_the_index_page_is_read_and_becomes_the_target() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut app = app_with_file(SettingsFeature::Index, "", &dir)?;
    let archive = folder(&dir, "Archive")?;

    settle(
        &mut app,
        settings(SettingsMessage::FolderPicked {
            kind: AddAs::Indexed,
            path: Some(archive.clone()),
        }),
    )
    .await;

    let location = single_location(&app)?;
    assert_eq!(location.path, archive);
    assert!(location.is_target);
    assert!(written(&dir)?.contains("Archive"));
    Ok(())
}

#[tokio::test]
async fn removing_the_target_hands_it_to_the_next_archive_and_the_last_folder_can_go() -> TestResult
{
    let dir = tempfile::tempdir()?;
    let mut app = app_with_file(SettingsFeature::Index, "", &dir)?;
    let first = folder(&dir, "First")?;
    let second = folder(&dir, "Second")?;
    for path in [&first, &second] {
        settle(
            &mut app,
            settings(SettingsMessage::FolderPicked {
                kind: AddAs::Indexed,
                path: Some(path.clone()),
            }),
        )
        .await;
    }
    assert_eq!(
        locations_of(&app)
            .iter()
            .map(|location| location.is_target)
            .collect::<Vec<_>>(),
        vec![true, false]
    );

    settle(&mut app, settings(SettingsMessage::RemoveFolder(first))).await;
    let remaining = locations_of(&app);
    assert_eq!(remaining.len(), 1);
    assert!(remaining.iter().all(|location| location.is_target));

    settle(&mut app, settings(SettingsMessage::RemoveFolder(second))).await;
    assert!(locations_of(&app).is_empty());
    Ok(())
}

#[tokio::test]
async fn a_read_folder_added_on_the_hermes_page_is_an_inbox_and_never_the_target() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut app = app_with_file(SettingsFeature::Hermes, "", &dir)?;
    let scans = folder(&dir, "Scans")?;

    settle(
        &mut app,
        settings(SettingsMessage::FolderPicked {
            kind: AddAs::ReadFolder,
            path: Some(scans),
        }),
    )
    .await;

    let location = single_location(&app)?;
    assert_eq!(location.mode, LocationMode::Inbox);
    assert!(!location.is_target);
    Ok(())
}

#[tokio::test]
async fn hermes_stays_off_when_a_read_folder_is_removed() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut app = app_with_file(SettingsFeature::Hermes, "", &dir)?;
    let scans = folder(&dir, "Scans")?;
    settle(
        &mut app,
        settings(SettingsMessage::FolderPicked {
            kind: AddAs::ReadFolder,
            path: Some(scans.clone()),
        }),
    )
    .await;

    settle(&mut app, settings(SettingsMessage::RemoveFolder(scans))).await;

    let config = from_text(&written(&dir)?)?;
    assert!(!config.filing.enabled);
    assert!(config.filing.locations.is_empty());
    Ok(())
}

#[tokio::test]
async fn the_dialog_writes_when_and_images_of_a_read_folder() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut app = app_with_file(SettingsFeature::Hermes, "", &dir)?;
    let scans = folder(&dir, "Scans")?;
    settle(
        &mut app,
        settings(SettingsMessage::FolderPicked {
            kind: AddAs::ReadFolder,
            path: Some(scans),
        }),
    )
    .await;
    let mut edited = locations_of(&app).remove(0);
    edited.mode = LocationMode::Archive;
    edited.images = true;

    settle(&mut app, settings(SettingsMessage::ReplaceFolder(edited))).await;

    let location = single_location(&app)?;
    assert_eq!(location.mode, LocationMode::Archive);
    assert!(location.images);
    Ok(())
}

#[tokio::test]
async fn a_rule_violation_shows_as_a_note_and_leaves_the_file() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut app = app_with_file(SettingsFeature::Index, "", &dir)?;
    let outer = folder(&dir, "Outer")?;
    let inner = outer.join("Inner");
    std::fs::create_dir(&inner)?;
    settle(
        &mut app,
        settings(SettingsMessage::FolderPicked {
            kind: AddAs::Indexed,
            path: Some(outer),
        }),
    )
    .await;
    let before = written(&dir)?;

    settle(
        &mut app,
        settings(SettingsMessage::FolderPicked {
            kind: AddAs::Indexed,
            path: Some(inner),
        }),
    )
    .await;

    assert_eq!(written(&dir)?, before);
    assert!(app
        .settings
        .page
        .as_ref()
        .is_some_and(|state| state.folder_note.is_some()));
    Ok(())
}

#[test]
fn changing_an_area_in_the_chat_opens_the_chat() {
    let mut app = App::with_settings_open(SettingsFeature::Hermes, Default::default());
    let area = AreaFacts {
        name: "Steuern".to_owned(),
        files: 12,
        read_folder: PathBuf::from("/home/test/Scans"),
        root: PathBuf::from("/home/test/Scans/Steuern"),
        order: String::new(),
    };

    drop(app.update(settings(SettingsMessage::OpenChat(Some(area)))));

    assert!(app.hermes_chat.open);
    assert!(app.settings.page.is_none());
}

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
    assert_eq!(messages, [settings(SettingsMessage::OpenFilingStatus)]);

    drop(app.update(settings(SettingsMessage::OpenFilingStatus)));
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
    assert_eq!(messages, vec![settings(SettingsMessage::UndoRun(2))]);
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

fn single_location(app: &App) -> Result<crate::config::FilingLocation, Box<dyn Error>> {
    let mut locations = locations_of(app);
    if locations.len() != 1 {
        return Err(format!("expected one location, found {}", locations.len()).into());
    }
    Ok(locations.remove(0))
}

#[tokio::test]
async fn the_plus_of_an_own_detail_adds_every_comma_or_semicolon_separated_entry() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut app = app_with_file(SettingsFeature::Index, "", &dir)?;
    let field = SettingField::Own(OwnDetail::Names);

    settle(
        &mut app,
        settings(SettingsMessage::Edit {
            field,
            text: "Max Muster; Muster GmbH, Max Muster".to_owned(),
        }),
    )
    .await;
    settle(
        &mut app,
        settings(SettingsMessage::AddOwnDetail(OwnDetail::Names)),
    )
    .await;

    let config = from_text(&written(&dir)?)?;
    assert_eq!(config.filing.own_names, ["Max Muster", "Muster GmbH"]);
    Ok(())
}
