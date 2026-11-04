use std::num::NonZeroUsize;

use crate::app::*;
use crate::config::FilingLocation;
use ncrs_index::InMemoryKeyStore;
use ncrs_intake::LocationMode;

fn location(path: &str, mode: LocationMode, is_target: bool) -> FilingLocation {
    FilingLocation {
        path: PathBuf::from(path),
        mode,
        images: false,
        is_target,
        transfer: Default::default(),
    }
}

fn saved_filing() -> Filing {
    Filing {
        enabled: true,
        locations: vec![
            location("/home/test/inbox", LocationMode::Inbox, false),
            location("/home/test/target", LocationMode::Archive, true),
        ],
        load: Load::default(),
        ai: crate::config::AiSetting::Off,
        default_pattern: None,
        own_names: Vec::new(),
        own_ibans: Vec::new(),
        own_tax_ids: Vec::new(),
        first_run: false,
    }
}

fn running_pipeline() -> ncrs_pipeline::Runner {
    ncrs_pipeline::Runner::start(
        ncrs_pipeline::Settings::new(Load::Gentle, NonZeroUsize::MIN),
        Vec::new(),
        Vec::new(),
        Arc::new(ncrs_pipeline::SystemClock),
    )
}

/// An app that can start a module: a store, but no data directory, so a
/// started module fails at once instead of opening an index.
fn app_that_can_start_a_module() -> App {
    let (app, _start) = App::with_fixed_panels().with_filing(
        &Filing::default(),
        None,
        Arc::new(InMemoryKeyStore::empty()),
    );
    assert!(app.hermes.filing.is_none(), "the module is off");
    app
}

#[test]
fn a_saved_list_starts_the_module_when_none_was_on() {
    let mut app = app_that_can_start_a_module();

    let (applies, _task) = app.apply_saved_filing(saved_filing());

    assert!(matches!(applies, SavedFilingApplies::Now));
    assert!(app
        .hermes
        .filing
        .as_ref()
        .unwrap()
        .has_configured_locations());
    assert_eq!(app.hermes.filing_config, saved_filing());
}

#[test]
fn a_module_without_locations_is_replaced_and_its_runner_stopped() {
    let mut app = app_that_can_start_a_module();
    app.hermes.filing = Some(FilingModule::in_phase(Phase::Ready));
    app.hermes.pipeline = Some(running_pipeline());

    let (applies, _task) = app.apply_saved_filing(saved_filing());

    assert!(matches!(applies, SavedFilingApplies::Now));
    assert!(app.hermes.pipeline.is_none());
    assert!(app
        .hermes
        .filing
        .as_ref()
        .unwrap()
        .has_configured_locations());
}

fn ai_filing() -> Filing {
    Filing {
        ai: crate::config::AiSetting::Local { threads: None },
        ..saved_filing()
    }
}

#[tokio::test]
async fn an_ai_change_rebuilds_the_pipeline_and_a_stale_answer_changes_nothing() {
    let mut app = app_that_can_start_a_module();
    app.hermes.filing = Some(FilingModule::in_phase_with_locations(Phase::Ready));
    app.hermes.pipeline = Some(running_pipeline());

    let (applies, _task) = app.apply_ai_change(ai_filing());

    assert!(matches!(applies, SavedFilingApplies::Now));
    assert!(app.hermes.pipeline.is_none() && app.hermes.filing.is_none());
    let (generation, _) = app
        .hermes
        .filing_restart
        .clone()
        .expect("a restart is pending");
    drop(app.filing_restarted(generation.wrapping_add(1)));
    assert!(
        app.hermes.filing_restart.is_some(),
        "a stale answer is ignored"
    );
    drop(app.filing_restarted(generation));
    assert!(app.hermes.filing_restart.is_none());
}

#[tokio::test]
async fn an_ai_change_waits_for_the_next_start_while_a_job_runs() {
    let mut app = app_that_can_start_a_module();
    app.hermes.filing = Some(FilingModule::in_phase_with_locations(Phase::Ready));
    app.hermes.pipeline = Some(running_pipeline());
    app.jobs.enqueue(jobs::Job {
        kind: jobs::JobKind::File,
        path: PathBuf::new(),
        total: None,
    });

    let (applies, _task) = app.apply_ai_change(ai_filing());

    assert!(matches!(applies, SavedFilingApplies::AtNextStart));
    assert!(app.hermes.pipeline.is_some() && app.hermes.filing.is_some());
    assert!(app.hermes.filing_restart.is_none());
}

#[test]
fn a_new_module_files_its_first_plans_even_after_an_older_runner_made_many() {
    let mut app = app_that_can_start_a_module();
    app.hermes.plans_filed_for = 7;

    drop(app.start_filing_module(&saved_filing()));

    assert_eq!(app.hermes.plans_filed_for, 0);
}

#[tokio::test]
async fn a_running_module_with_locations_keeps_them_until_the_next_start() {
    let mut app = app_that_can_start_a_module();
    app.hermes.filing = Some(FilingModule::in_phase_with_locations(Phase::Ready));
    app.hermes.pipeline = Some(running_pipeline());

    let (applies, _task) = app.apply_saved_filing(saved_filing());

    assert!(matches!(applies, SavedFilingApplies::AtNextStart));
    assert!(app.hermes.pipeline.is_some());
    assert!(matches!(
        app.hermes.filing.as_ref().unwrap().phase(),
        Phase::Ready
    ));
    assert_eq!(app.hermes.filing_config, saved_filing());
}

#[test]
fn a_module_busy_with_its_start_is_not_replaced() {
    let mut app = app_that_can_start_a_module();
    app.hermes.filing = Some(FilingModule::in_phase(Phase::Starting));

    let (applies, _task) = app.apply_saved_filing(saved_filing());

    assert!(matches!(applies, SavedFilingApplies::AtNextStart));
    assert!(matches!(
        app.hermes.filing.as_ref().unwrap().phase(),
        Phase::Starting
    ));
}
