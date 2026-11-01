use std::num::NonZeroUsize;
use std::time::SystemTime;

use crate::app::*;
use crate::filing_status::INDEX_FIGURES_EVERY_TICKS;
use ncrs_pipeline::{RunnerState, Settings, Snapshot, SystemClock};

fn status(message: FilingStatusMessage) -> Message {
    Message::FilingStatus(message)
}

fn tick(app: &mut App) {
    drop(app.update(status(FilingStatusMessage::Tick)));
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

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
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
    assert_eq!(worker_target(&app), 2);

    app.hermes
        .set_pipeline_load(Load::Full, app.wall_clock.now());
    assert_eq!(worker_target(&app), 8);
    assert_eq!(app.hermes.filing.as_ref().unwrap().load(), Load::Full);

    app.hermes
        .set_pipeline_load(Load::Gentle, app.wall_clock.now());
    assert_eq!(worker_target(&app), 2);
    assert_eq!(app.hermes.filing.as_ref().unwrap().load(), Load::Gentle);
}

#[test]
fn stepping_the_workers_never_goes_below_one() {
    let mut app = app_with_empty_pipeline();

    drop(app.update(status(FilingStatusMessage::StepWorkers(-1))));
    assert_eq!(worker_target(&app), 1);
    assert_eq!(
        app.hermes.filing.as_ref().unwrap().load(),
        Load::Workers(NonZeroUsize::MIN)
    );

    drop(app.update(status(FilingStatusMessage::StepWorkers(-1))));
    assert_eq!(worker_target(&app), 1);
}

#[test]
fn without_a_pipeline_pause_and_load_do_nothing() {
    let mut app = App::with_fixed_filing(Phase::Ready);

    drop(app.update(status(FilingStatusMessage::TogglePause)));
    drop(app.update(status(FilingStatusMessage::StepWorkers(1))));

    assert!(app.hermes.pipeline.is_none());
    assert_eq!(app.hermes.filing.as_ref().unwrap().load(), Load::default());
}

#[test]
fn index_figures_are_asked_for_on_opening_and_then_every_fifth_tick() {
    let runtime = runtime();
    let _inside = runtime.enter();
    let mut app = App::with_fixed_filing(Phase::Ready);

    drop(app.open_filing_status());
    assert!(app.hermes.filing_status.figures_in_flight());

    for _ in 0..2 * INDEX_FIGURES_EVERY_TICKS {
        tick(&mut app);
    }
    assert!(app.hermes.filing_status.figures_in_flight());

    drop(app.update(status(FilingStatusMessage::IndexFigures(Err(
        "index not open".to_owned(),
    )))));
    assert!(!app.hermes.filing_status.figures_in_flight());
    for _ in 1..INDEX_FIGURES_EVERY_TICKS {
        tick(&mut app);
        assert!(!app.hermes.filing_status.figures_in_flight());
    }
    tick(&mut app);
    assert!(app.hermes.filing_status.figures_in_flight());
}

#[test]
fn index_figures_are_never_asked_for_while_the_page_is_closed() {
    let runtime = runtime();
    let _inside = runtime.enter();
    let mut app = App::with_fixed_filing(Phase::Ready);

    for _ in 0..3 * INDEX_FIGURES_EVERY_TICKS {
        tick(&mut app);
    }
    assert!(!app.hermes.filing_status.figures_in_flight());

    drop(app.open_filing_status());
    drop(app.update(status(FilingStatusMessage::Close)));
    drop(app.update(status(FilingStatusMessage::IndexFigures(Err(
        "x".to_owned(),
    )))));
    for _ in 0..3 * INDEX_FIGURES_EVERY_TICKS {
        tick(&mut app);
    }
    assert!(!app.hermes.filing_status.figures_in_flight());
}

#[test]
fn opening_the_page_is_refused_under_another_overlay() {
    let mut app = App::with_fixed_prompt();

    drop(app.update(status(FilingStatusMessage::Open)));

    assert!(!app.hermes.filing_status.page_open());
}

#[test]
fn the_key_opens_status_and_errors_from_the_panels() {
    let mut app = App::with_fixed_panels();

    drop(app.update(status(FilingStatusMessage::Toggle)));

    assert_eq!(app.open_hermes_page(), Some(HermesPage::Inspection));
}

#[test]
fn the_key_goes_back_to_the_panels_from_status_and_errors() {
    let mut app = App::with_filing_status_page(Snapshot::off(), SystemTime::UNIX_EPOCH);

    drop(app.update(status(FilingStatusMessage::Toggle)));

    assert_eq!(app.open_hermes_page(), None);
}

#[test]
fn the_key_switches_to_status_and_errors_from_the_overview() {
    let mut app = App::with_fixed_panels();
    drop(app.update(status(FilingStatusMessage::Open)));
    assert_eq!(app.open_hermes_page(), Some(HermesPage::Overview));

    drop(app.update(status(FilingStatusMessage::Toggle)));

    assert_eq!(app.open_hermes_page(), Some(HermesPage::Inspection));
}

#[test]
fn the_page_owns_the_keys_while_it_is_open() {
    let open = PromptKeyState {
        filing_status: true,
        ..PromptKeyState::default()
    };
    let route = |key| route_key(&open, key, Modifiers::default());

    assert_eq!(
        route(Key::Named(Named::Escape)),
        Some(status(FilingStatusMessage::Close))
    );
    assert_eq!(
        route(Key::Character("p".into())),
        Some(status(FilingStatusMessage::TogglePause))
    );
    assert_eq!(
        route(Key::Character("2".into())),
        Some(Message::HermesOverview(HermesOverviewMessage::SetLoad(
            Load::Balanced
        )))
    );
    assert_eq!(
        route(Key::Named(Named::ArrowDown)),
        Some(status(FilingStatusMessage::MoveError(1)))
    );
    assert_eq!(route(Key::Named(Named::F5)), None);
    for chord in [Modifiers::COMMAND, Modifiers::CTRL, Modifiers::ALT] {
        for character in ["p", "1", "+"] {
            assert_eq!(
                route_key(&open, Key::Character(character.into()), chord),
                None,
                "{chord:?}+{character}"
            );
        }
    }
}

#[test]
fn the_binding_toggles_the_page_and_closes_it_again() {
    let chord = Modifiers::COMMAND | Modifiers::SHIFT;
    let key = || Key::Character("d".into());

    assert_eq!(
        route_key(&PromptKeyState::default(), key(), chord),
        Some(status(FilingStatusMessage::Toggle))
    );
    let open = PromptKeyState {
        filing_status: true,
        ..PromptKeyState::default()
    };
    assert_eq!(
        route_key(&open, key(), chord),
        Some(status(FilingStatusMessage::Close))
    );
}

#[test]
fn an_error_row_closes_the_page_and_shows_the_file_in_the_active_panel() {
    let mut app = App::with_filing_status_page(Snapshot::off(), SystemTime::UNIX_EPOCH);
    let before = app.panel(PanelSide::Left).request_id;

    drop(
        app.update(status(FilingStatusMessage::ErrorActivated(PathBuf::from(
            "/tmp/inbox/company-a-invoice.pdf",
        )))),
    );

    assert!(!app.hermes.filing_status.page_open());
    assert!(app.panel(PanelSide::Left).loading);
    assert_ne!(app.panel(PanelSide::Left).request_id, before);
}

#[test]
fn enter_reads_the_errors_again_and_space_checks_the_selected_one() {
    let open = PromptKeyState {
        filing_status: true,
        ..PromptKeyState::default()
    };
    assert_eq!(
        route_key(&open, Key::Named(Named::Enter), Modifiers::empty()),
        Some(status(FilingStatusMessage::ProcessAgain))
    );
    assert_eq!(
        route_key(&open, Key::Named(Named::Space), Modifiers::empty()),
        Some(status(FilingStatusMessage::ToggleSelectedErrorChecked))
    );

    let mut app = App::with_filing_status_page(Snapshot::off(), SystemTime::UNIX_EPOCH);
    drop(app.update(status(FilingStatusMessage::ProcessAgain)));
    assert!(app.hermes.filing_status.page_open());
}
