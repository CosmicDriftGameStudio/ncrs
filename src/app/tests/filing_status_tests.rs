use std::num::NonZeroUsize;
use std::time::SystemTime;

use crate::app::*;
use crate::messages::SettingsFeature;
use ncrs_pipeline::{RunnerState, Settings, Snapshot, SystemClock};

fn status(message: FilingStatusMessage) -> Message {
    Message::FilingStatus(message)
}

fn close_settings_with_escape(app: &mut App) {
    drop(app.update(Message::Settings(SettingsMessage::Close)));
}

/// An app whose pipeline has no steps and no folders, standing on 8 cores.
fn app_with_empty_pipeline() -> App {
    let mut app = App::with_fixed_filing(Phase::Ready);
    app.hermes.pipeline = Some(ncrs_pipeline::Runner::start(
        Settings::new(Load::Gentle, NonZeroUsize::new(8).unwrap()),
        Vec::new(),
        Vec::new(),
        Arc::new(SystemClock),
    ));
    app
}

fn worker_target(app: &App) -> usize {
    app.hermes
        .pipeline
        .as_ref()
        .unwrap()
        .snapshot()
        .worker_target
}

#[test]
fn pause_and_resume_act_on_the_pipeline_at_once() {
    let mut app = app_with_empty_pipeline();

    drop(app.update(status(FilingStatusMessage::TogglePause)));
    assert_eq!(
        app.hermes.pipeline.as_ref().unwrap().snapshot().state,
        RunnerState::Paused
    );

    drop(app.update(status(FilingStatusMessage::TogglePause)));
    assert_ne!(
        app.hermes.pipeline.as_ref().unwrap().snapshot().state,
        RunnerState::Paused
    );
}

#[test]
fn the_load_is_set_on_the_pipeline_and_remembered_by_the_module() {
    let mut app = app_with_empty_pipeline();
    assert_eq!(worker_target(&app), 1);

    app.hermes
        .set_pipeline_load(Load::Full, app.wall_clock.now());
    assert_eq!(worker_target(&app), 4);
    assert_eq!(app.hermes.filing.as_ref().unwrap().load(), Load::Full);

    app.hermes
        .set_pipeline_load(Load::Gentle, app.wall_clock.now());
    assert_eq!(worker_target(&app), 1);
    assert_eq!(app.hermes.filing.as_ref().unwrap().load(), Load::Gentle);
}

#[test]
fn without_a_pipeline_the_pause_does_nothing() {
    let mut app = App::with_fixed_filing(Phase::Ready);

    drop(app.update(status(FilingStatusMessage::TogglePause)));

    assert!(app.hermes.pipeline.is_none());
    assert_eq!(app.hermes.filing.as_ref().unwrap().load(), Load::default());
}

#[test]
fn the_status_key_and_the_chip_open_the_settings_on_the_read_errors() {
    for message in [FilingStatusMessage::Toggle, FilingStatusMessage::Open] {
        let mut app = App::with_fixed_panels();

        drop(app.update(status(message)));

        assert_eq!(app.open_hermes_page(), Some(HermesPage::Inspection));
        assert_eq!(
            settings_ui::shown_feature(&app.settings_view(app.settings.page.as_ref().unwrap())),
            Some(SettingsFeature::ReadErrors)
        );
    }
}

#[test]
fn the_status_opens_no_second_tab_and_escape_closes_the_settings() {
    let mut app = App::with_filing_status_page(Snapshot::off(), SystemTime::UNIX_EPOCH);

    drop(app.update(status(FilingStatusMessage::Open)));
    assert_eq!(app.open_hermes_page(), Some(HermesPage::Inspection));

    close_settings_with_escape(&mut app);

    assert!(app.settings.page.is_none());
    assert_eq!(app.open_hermes_page(), None);
    assert!(!app.hermes.filing_status.page_open());
}

#[test]
fn an_error_row_closes_the_settings_and_shows_the_file_in_the_active_panel() {
    let mut app = App::with_filing_status_page(Snapshot::off(), SystemTime::UNIX_EPOCH);
    let before = app.panel(PanelSide::Left).request_id;

    drop(
        app.update(status(FilingStatusMessage::ErrorActivated(PathBuf::from(
            "/tmp/inbox/company-a-invoice.pdf",
        )))),
    );

    assert!(app.settings.page.is_none());
    assert!(!app.hermes.filing_status.page_open());
    assert!(app.panel(PanelSide::Left).loading);
    assert_ne!(app.panel(PanelSide::Left).request_id, before);
}
