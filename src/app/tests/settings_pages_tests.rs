#![allow(
    clippy::panic_in_result_fn,
    reason = "a failing assertion is the signal in a test"
)]

use ncrs_settings::message::SettingField;
use std::error::Error;

use iced_test::futures::futures::StreamExt as _;
use iced_test::runtime::{task, Action};
use ncrs_settings::folders::AddAs;

use crate::app::*;
use crate::config::{from_text, OwnDetail};
use crate::messages::SettingsFeature;

pub(in crate::app::tests) type TestResult = Result<(), Box<dyn Error>>;

/// Delivers `message` and everything the tasks it starts report.
pub(in crate::app::tests) async fn settle(app: &mut App, message: Message) {
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

pub(in crate::app::tests) fn settings(change: SettingsMessage) -> Message {
    Message::Settings(change)
}

/// The settings open on `feature`, showing and writing the config file that
/// holds `source`.
pub(in crate::app::tests) fn app_with_file(
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

pub(in crate::app::tests) fn written(dir: &tempfile::TempDir) -> Result<String, Box<dyn Error>> {
    Ok(std::fs::read_to_string(dir.path().join("config.toml"))?)
}

fn folder(dir: &tempfile::TempDir, name: &str) -> Result<std::path::PathBuf, Box<dyn Error>> {
    let path = dir.path().join(name);
    std::fs::create_dir(&path)?;
    Ok(ncrs_intake::canonical_path(&path)?)
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
async fn a_folder_added_on_the_index_page_is_only_read_and_never_a_target() -> TestResult {
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
    assert!(!location.is_target);
    assert_eq!(location.mode, ncrs_intake::LocationMode::Archive);
    assert!(written(&dir)?.contains("Archive"));
    Ok(())
}

#[tokio::test]
async fn removing_a_folder_leaves_the_others_and_the_last_folder_can_go() -> TestResult {
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
    assert_eq!(locations_of(&app).len(), 2);

    settle(&mut app, settings(SettingsMessage::RemoveFolder(first))).await;
    let remaining = single_location(&app)?;
    assert_eq!(remaining.path, second);
    assert!(!remaining.is_target);

    settle(&mut app, settings(SettingsMessage::RemoveFolder(second))).await;
    assert!(locations_of(&app).is_empty());
    Ok(())
}

#[tokio::test]
async fn hermes_stays_off_when_a_folder_is_removed() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut app = app_with_file(SettingsFeature::Index, "", &dir)?;
    let scans = folder(&dir, "Scans")?;
    settle(
        &mut app,
        settings(SettingsMessage::FolderPicked {
            kind: AddAs::Indexed,
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
