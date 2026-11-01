use std::num::NonZeroUsize;

use crate::app::*;
use crate::config::FilingLocation;
use crate::filing_setup::SetupProblem;
use ncrs_index::InMemoryKeyStore;
use ncrs_intake::LocationMode;

fn setup_message(message: FilingSetupMessage) -> Message {
    Message::FilingSetup(message)
}

fn location(path: &str, mode: LocationMode, is_target: bool) -> FilingLocation {
    FilingLocation {
        path: PathBuf::from(path),
        mode,
        images: false,
        is_target,
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
    let (mut app, _start) = App::with_fixed_panels().with_filing(
        &Filing::default(),
        None,
        Arc::new(InMemoryKeyStore::empty()),
    );
    assert!(app.hermes.filing.is_none(), "the module is off");
    app.hermes.filing_setup = Some(FilingSetup::new(&[]));
    app
}

fn note(app: &App) -> Option<&SetupNote> {
    app.hermes.filing_setup.as_ref().and_then(FilingSetup::note)
}

#[test]
fn the_setup_key_opens_the_setup_with_the_configured_locations_and_closes_it() {
    let mut app = App::with_fixed_panels();
    app.hermes.filing_config = saved_filing();

    drop(app.update(setup_message(FilingSetupMessage::Toggle)));
    assert_eq!(
        app.hermes.filing_setup.as_ref().unwrap().chosen(),
        saved_filing().locations
    );

    drop(app.update(setup_message(FilingSetupMessage::Toggle)));
    assert!(app.hermes.filing_setup.is_none());
}

#[test]
fn escape_closes_the_setup_when_nothing_else_wants_it() {
    let mut app = App::with_filing_setup(&[], false);
    let escape = route_key(
        &app.prompt_key_state(),
        Key::Named(Named::Escape),
        Modifiers::default(),
    );
    assert_eq!(escape, Some(setup_message(FilingSetupMessage::Close)));

    drop(app.update(escape.unwrap()));
    assert!(app.hermes.filing_setup.is_none());
    assert_eq!(
        route_key(
            &app.prompt_key_state(),
            Key::Named(Named::Escape),
            Modifiers::default()
        ),
        None
    );
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
}

fn pick(app: &mut App, mode: LocationMode, path: &str) {
    drop(app.update(setup_message(FilingSetupMessage::PickFolder(mode))));
    assert!(app.hermes.filing_setup.as_ref().unwrap().is_busy());
    let folder = PathBuf::from(path);
    drop(app.update(setup_message(FilingSetupMessage::FolderPicked {
        mode,
        folder: Some(folder.clone()),
    })));
    drop(app.update(setup_message(FilingSetupMessage::Added {
        mode,
        folder: folder.clone(),
        resolved: Ok(folder),
    })));
    assert!(!app.hermes.filing_setup.as_ref().unwrap().is_busy());
}

#[test]
fn folders_from_the_dialog_fill_both_sections_without_touching_the_panels() {
    let runtime = runtime();
    let _inside = runtime.enter();
    let mut app = App::with_filing_setup(&[], false);
    let panel_folder = app.active_panel().path.clone();

    pick(&mut app, LocationMode::Inbox, "/home/test/Downloads/Scans");
    pick(&mut app, LocationMode::Inbox, "/home/test/Scanner");
    pick(
        &mut app,
        LocationMode::Archive,
        "/home/test/Documents/Filed",
    );
    pick(
        &mut app,
        LocationMode::Archive,
        "/home/test/Documents/Old-2019",
    );

    let setup = app.hermes.filing_setup.as_ref().unwrap();
    let count = |mode| setup.chosen().iter().filter(|l| l.mode == mode).count();
    assert_eq!(count(LocationMode::Inbox), 2);
    assert_eq!(count(LocationMode::Archive), 2);
    assert_eq!(app.active_panel().path, panel_folder);
    assert_eq!(setup.problem(), Some(SetupProblem::ChooseFilingLocation));

    let filed = setup
        .chosen()
        .iter()
        .position(|l| l.path == Path::new("/home/test/Documents/Filed"))
        .unwrap();
    drop(app.update(setup_message(FilingSetupMessage::SetTarget(filed))));
    assert!(app.hermes.filing_setup.as_ref().unwrap().can_save());
}

#[test]
fn a_cancelled_dialog_adds_nothing() {
    let mut app = App::with_filing_setup(&[], false);

    drop(app.update(setup_message(FilingSetupMessage::PickFolder(
        LocationMode::Inbox,
    ))));
    assert!(app.hermes.filing_setup.as_ref().unwrap().is_busy());
    drop(app.update(setup_message(FilingSetupMessage::FolderPicked {
        mode: LocationMode::Inbox,
        folder: None,
    })));

    let setup = app.hermes.filing_setup.as_ref().unwrap();
    assert!(!setup.is_busy());
    assert!(setup.chosen().is_empty());
}

#[test]
fn the_same_folder_twice_gives_a_note_and_adds_nothing() {
    let runtime = runtime();
    let _inside = runtime.enter();
    let mut app = App::with_filing_setup(&[], false);

    pick(
        &mut app,
        LocationMode::Archive,
        "/home/test/Documents/Filed",
    );
    pick(
        &mut app,
        LocationMode::Archive,
        "/home/test/Documents/Filed",
    );

    assert_eq!(app.hermes.filing_setup.as_ref().unwrap().chosen().len(), 1);
    assert_eq!(
        note(&app),
        Some(&SetupNote::AlreadyAdded {
            folder: PathBuf::from("/home/test/Documents/Filed")
        })
    );
}

#[test]
fn the_panel_fallback_adds_the_active_panels_folder() {
    let runtime = runtime();
    let _inside = runtime.enter();
    let mut app = App::with_filing_setup(&[], false);
    let folder = app.active_panel().path.clone();

    drop(app.update(setup_message(FilingSetupMessage::AddPanelFolder(
        LocationMode::Archive,
    ))));
    assert!(app.hermes.filing_setup.as_ref().unwrap().is_busy());

    drop(app.update(setup_message(FilingSetupMessage::Added {
        mode: LocationMode::Archive,
        folder: folder.clone(),
        resolved: Ok(folder.clone()),
    })));
    let setup = app.hermes.filing_setup.as_ref().unwrap();
    assert!(!setup.is_busy());
    assert_eq!(setup.chosen().len(), 1);
    assert_eq!(setup.chosen()[0].path, folder);
    assert_eq!(setup.chosen()[0].mode, LocationMode::Archive);
}

#[test]
fn saving_without_a_config_path_says_so_and_writes_nothing() {
    let mut app = App::with_filing_setup(&saved_filing().locations, false);

    drop(app.update(setup_message(FilingSetupMessage::Save)));

    assert_eq!(note(&app), Some(&SetupNote::NoConfigPath));
    assert!(!app.hermes.filing_setup.as_ref().unwrap().is_busy());
}

#[test]
fn a_saved_list_starts_the_module_when_none_was_on() {
    let mut app = app_that_can_start_a_module();

    drop(app.update(setup_message(FilingSetupMessage::Saved(Ok(saved_filing())))));

    assert!(app.hermes.filing_setup.is_none(), "saving closes the card");
    let key = app
        .keymap
        .shortcut_label(&Message::FilingStatus(FilingStatusMessage::Toggle))
        .unwrap();
    let notice = app.notice.as_deref().unwrap();
    assert!(notice.contains("Filing is on"), "{notice}");
    assert!(notice.contains(&key), "{notice}");
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

    drop(app.update(setup_message(FilingSetupMessage::Saved(Ok(saved_filing())))));

    assert!(app.hermes.filing_setup.is_none());
    assert!(app.notice.as_deref().unwrap().contains("Filing is on"));
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

    drop(app.update(setup_message(FilingSetupMessage::Saved(Ok(saved_filing())))));

    assert!(app.hermes.filing_setup.is_none());
    assert_eq!(
        app.notice.as_deref(),
        Some(app.lang.text(Msg::FilingSetupSavedNextStart))
    );
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

    drop(app.update(setup_message(FilingSetupMessage::Saved(Ok(saved_filing())))));

    assert!(app.hermes.filing_setup.is_none());
    assert_eq!(
        app.notice.as_deref(),
        Some(app.lang.text(Msg::FilingSetupSavedNextStart))
    );
    assert!(matches!(
        app.hermes.filing.as_ref().unwrap().phase(),
        Phase::Starting
    ));
}

#[test]
fn a_failed_save_is_shown_and_changes_nothing() {
    let mut app = app_that_can_start_a_module();
    let failure = FilingWriteError::Rule(crate::config::FilingRuleViolation::NoTarget);

    drop(app.update(setup_message(FilingSetupMessage::Saved(Err(
        failure.clone(),
    )))));

    assert_eq!(note(&app), Some(&SetupNote::SaveFailed(failure)));
    assert!(app.hermes.filing.is_none());
    assert_eq!(app.hermes.filing_config, Filing::default());
}

#[test]
fn switched_on_without_locations_the_setup_opens_at_start() {
    let store = || -> Arc<dyn KeyStore + Send + Sync> { Arc::new(InMemoryKeyStore::empty()) };
    let without_locations = Filing {
        enabled: true,
        locations: Vec::new(),
        load: Load::default(),
        ai: crate::config::AiSetting::Off,
        default_pattern: None,
        own_names: Vec::new(),
        own_ibans: Vec::new(),
        own_tax_ids: Vec::new(),
    };
    let (empty, _empty_start) =
        App::with_fixed_panels().with_filing(&without_locations, None, store());
    assert!(empty.hermes.hermes_setup.is_some());
    assert!(empty.hermes.filing_setup.is_none());

    let (configured, _configured_start) =
        App::with_fixed_panels().with_filing(&saved_filing(), None, store());
    assert!(configured.hermes.filing_setup.is_none() && configured.hermes.hermes_setup.is_none());

    let (off, _off_start) = App::with_fixed_panels().with_filing(&Filing::default(), None, store());
    assert!(off.hermes.filing_setup.is_none() && off.hermes.hermes_setup.is_none());
}
