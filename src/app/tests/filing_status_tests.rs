use std::num::NonZeroUsize;
use std::time::SystemTime;

use crate::app::*;
use crate::messages::{ActivityMessage, SettingsFeature};
use iced_test::futures::subscription::into_recipes;
use ncrs_config::config::SourceSetting;
use ncrs_hermes_ui::filing_status::SourceQuestions;
use ncrs_index::TargetId;
use ncrs_intake::SortSource;
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
fn the_status_key_and_the_chip_open_the_activity_page() {
    let mut app = App::with_fixed_panels();

    drop(app.update(status(FilingStatusMessage::Open)));

    assert_eq!(app.open_hermes_page(), Some(HermesPage::Activity));
    assert_eq!(
        settings_ui::shown_feature(&app.settings_view(app.settings.page.as_ref().unwrap())),
        Some(SettingsFeature::Activity)
    );
}

fn watched_source(path: &str) -> SourceSetting {
    SourceSetting {
        source: SortSource {
            path: PathBuf::from(path),
            include: String::new(),
            exclude: String::new(),
            targets: vec![TargetId::main()],
            watch: true,
            transfer: Default::default(),
            no_text: Default::default(),
            several: Default::default(),
            own_volume: None,
        },
        paused: None,
    }
}

/// An app with two watched sources, of which those in `questions` wait for
/// answers.
fn app_with_questions(questions: &[(&str, usize)]) -> App {
    let mut app = App::with_fixed_filing(Phase::Ready);
    app.hermes.filing_config.sources = vec![
        watched_source("/home/test/Scans"),
        watched_source("/home/test/Mail"),
    ];
    let asked = questions
        .iter()
        .map(|(source, count)| SourceQuestions {
            source: PathBuf::from(source),
            count: *count,
        })
        .collect();
    app.hermes.filing_status.questions_arrived(Ok(asked));
    let mut idle = Snapshot::off();
    idle.state = RunnerState::Idle;
    let now = app.wall_clock.now();
    app.hermes.filing_status.set_snapshot(idle, now);
    app
}

fn press_badge(app: &mut App) {
    let pressed = app.filing_badge().expect("the counter").on_press;
    drop(app.update(pressed));
}

#[tokio::test]
async fn the_counter_opens_the_plan_of_the_only_source_that_asks() {
    let mut app = app_with_questions(&[("/home/test/Scans", 3)]);

    press_badge(&mut app);

    assert_eq!(app.shown_tab, ShownTab::Filing);
    let session = app.filing_view.session.as_ref().expect("the plan");
    assert_eq!(session.source, watched_source("/home/test/Scans").source);
    assert!(!session.once);
}

#[tokio::test]
async fn the_counter_opens_the_sources_when_several_ask() {
    let mut app = app_with_questions(&[("/home/test/Scans", 2), ("/home/test/Mail", 1)]);

    press_badge(&mut app);

    assert_eq!(app.open_hermes_page(), Some(HermesPage::Sorting));
    assert!(app.filing_view.session.is_none());
}

#[tokio::test]
async fn the_counter_opens_the_activity_page_when_nothing_asks() {
    let mut app = app_with_questions(&[]);

    press_badge(&mut app);

    assert_eq!(app.open_hermes_page(), Some(HermesPage::Activity));
}

fn stream_count(app: &App) -> usize {
    into_recipes(app.subscription()).len()
}

#[tokio::test]
async fn an_open_plan_keeps_no_stream_running_once_it_is_made() {
    let mut app = app_with_questions(&[("/home/test/Scans", 3)]);
    let before = stream_count(&app);

    press_badge(&mut app);
    assert_eq!(stream_count(&app), before + 1, "the plan is being made");

    let session = app.filing_view.session.as_ref().expect("the plan").id;
    drop(
        app.update(Message::FilingView(ncrs_filing_view::Message::Planned {
            session,
            read: false,
            result: Err("the index is gone".to_owned()),
        })),
    );
    assert_eq!(stream_count(&app), before);
}

#[tokio::test]
async fn the_activity_page_leads_to_what_waits_and_to_the_read_errors() {
    let mut app = app_with_questions(&[("/home/test/Scans", 2), ("/home/test/Mail", 1)]);
    drop(app.update(status(FilingStatusMessage::Open)));

    drop(app.update(Message::Activity(ActivityMessage::OpenWaiting)));
    assert_eq!(app.open_hermes_page(), Some(HermesPage::Sorting));

    drop(app.update(Message::Activity(ActivityMessage::OpenReadErrors)));
    assert_eq!(app.open_hermes_page(), Some(HermesPage::Inspection));
    assert_eq!(
        settings_ui::shown_feature(&app.settings_view(app.settings.page.as_ref().unwrap())),
        Some(SettingsFeature::ReadErrors)
    );
}

#[test]
fn the_status_opens_no_second_tab_and_escape_closes_the_settings() {
    let mut app = App::with_filing_status_page(Snapshot::off(), SystemTime::UNIX_EPOCH);

    drop(app.update(status(FilingStatusMessage::Open)));
    assert_eq!(app.open_hermes_page(), Some(HermesPage::Activity));

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
