use crate::app::*;

const BASE_THEME: &str = r##"{"colors": {"editor.background": "#102030"}}"##;

fn picked_in(outside: &TempDir, name: &str, text: &str) -> PathBuf {
    let path = outside.path().join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&path, text).unwrap();
    path
}

fn copied(config_dir: &TempDir, name: &str) -> PathBuf {
    config_dir.path().join("themes").join("picked").join(name)
}

#[test]
fn a_picked_theme_file_is_copied_next_to_the_config_for_the_sandbox() {
    let config_dir = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let picked = picked_in(&outside, "night.json", BASE_THEME);
    let config_path = config_dir.path().join("config.toml");

    let copy = copy_picked_theme_file(&config_path, &picked).unwrap();

    assert_eq!(copy.entry, "themes/picked/night.json");
    assert!(copy.include_not_copied.is_none());
    assert_eq!(
        std::fs::read(copied(&config_dir, "night.json")).unwrap(),
        BASE_THEME.as_bytes()
    );
}

#[test]
fn the_included_file_is_copied_along_and_the_theme_loads_with_its_colors() {
    let config_dir = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let picked = picked_in(
        &outside,
        "night.json",
        r#"{"include": "base/colors.json", "colors": {}}"#,
    );
    picked_in(&outside, "base/colors.json", BASE_THEME);
    let config_path = config_dir.path().join("config.toml");

    let copy = copy_picked_theme_file(&config_path, &picked).unwrap();

    assert!(copy.include_not_copied.is_none());
    assert_eq!(
        std::fs::read(copied(&config_dir, "base/colors.json")).unwrap(),
        BASE_THEME.as_bytes()
    );
    let loaded = crate::vscode_theme::VscodeTheme::load(&copied(&config_dir, "night.json"));
    assert_eq!(loaded.problem, None);
    let mut palette = Palette::default();
    loaded.apply(&mut palette);
    assert_eq!(palette.background, iced::Color::from_rgb8(0x10, 0x20, 0x30));
}

#[test]
fn an_include_outside_the_theme_folder_is_not_copied() {
    let outside = TempDir::new().unwrap();
    let absolute = outside
        .path()
        .join("abs.json")
        .to_string_lossy()
        .into_owned();
    for include in [
        "../outside.json",
        "a/../../outside.json",
        "/etc/x.json",
        absolute.as_str(),
    ] {
        let config_dir = TempDir::new().unwrap();
        let work = TempDir::new().unwrap();
        let picked = picked_in(
            &work,
            "theme/night.json",
            &format!(r#"{{"include": {include:?}, "colors": {{}}}}"#),
        );
        picked_in(&work, "outside.json", BASE_THEME);
        picked_in(&outside, "abs.json", BASE_THEME);
        let config_path = config_dir.path().join("config.toml");

        let copy = copy_picked_theme_file(&config_path, &picked).unwrap();

        let problem = copy.include_not_copied.expect(include);
        assert!(
            matches!(problem.reason, IncludeProblem::OutsideFolder),
            "{include}"
        );
        let names: Vec<_> = std::fs::read_dir(config_dir.path().join("themes/picked"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["night.json"], "{include}");
    }
}

#[test]
fn a_missing_include_is_reported_and_the_theme_is_still_copied() {
    let config_dir = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let picked = picked_in(
        &outside,
        "night.json",
        r#"{"include": "gone.json", "colors": {}}"#,
    );
    let config_path = config_dir.path().join("config.toml");

    let copy = copy_picked_theme_file(&config_path, &picked).unwrap();

    assert_eq!(copy.entry, "themes/picked/night.json");
    assert!(copied(&config_dir, "night.json").exists());
    let problem = copy.include_not_copied.unwrap();
    assert_eq!(problem.name, "gone.json");
    assert!(matches!(problem.reason, IncludeProblem::Unreadable(_)));
}

#[test]
fn an_uncopyable_include_shows_a_notice_in_the_sandbox() {
    let config_dir = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let picked = picked_in(
        &outside,
        "night.json",
        r#"{"include": "../outside.json", "colors": {}}"#,
    );
    let mut app = App::with_environment(Environment::isolated()).0;
    app.sandbox.active = true;
    app.config_path = Some(config_dir.path().join("config.toml"));

    drop(app.update_settings(SettingsMessage::ThemeFilePicked(Some(picked.clone()))));

    let notice = app.notice.expect("a notice");
    assert!(notice.contains("../outside.json"), "{notice}");
}

#[test]
fn an_unreadable_theme_file_is_reported_and_nothing_is_entered() {
    let config_dir = TempDir::new().unwrap();
    let missing = config_dir.path().join("gone.json");
    let config_path = config_dir.path().join("config.toml");
    copy_picked_theme_file(&config_path, &missing).unwrap_err();
}
use crate::config::FilingLocation;
use crate::filing::Phase;
use ncrs_intake::LocationMode;
use tempfile::TempDir;

fn location(path: &str, mode: LocationMode, is_target: bool) -> FilingLocation {
    FilingLocation {
        path: PathBuf::from(path),
        mode,
        images: false,
        is_target,
    }
}

fn chosen_folders() -> Vec<FilingLocation> {
    vec![
        location("/home/test/inbox", LocationMode::Inbox, false),
        location("/home/test/target", LocationMode::Archive, true),
    ]
}

fn filing_with(locations: Vec<FilingLocation>) -> Filing {
    Filing {
        enabled: true,
        locations,
        ..Filing::default()
    }
}

fn offer(model_download: bool) -> SetupOffer {
    SetupOffer {
        existing_index: None,
        model_download,
        hermes_was_off: false,
    }
}

fn asks_encryption() -> Phase {
    Phase::AskEncryption {
        encrypted: true,
        busy: false,
        error: None,
    }
}

/// The card is up on the two folders, a module waits at the question, and
/// the config lists the same folders.
fn app_at_the_question(index_dir: &TempDir) -> App {
    let mut app = App::with_hermes_setup(&chosen_folders(), offer(false));
    app.hermes.filing = Some(FilingModule::in_phase_with_locations_at(
        asks_encryption(),
        index_dir.path().join("index.db"),
    ));
    app
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
}

fn press_start(app: &mut App, config_dir: &TempDir) {
    app.config_path = Some(config_dir.path().join("config.toml"));
    drop(app.update(Message::HermesSetup(HermesSetupMessage::Start)));
}

#[test]
fn start_does_nothing_until_both_folders_are_chosen() {
    let config_dir = TempDir::new().unwrap();
    let mut app = App::with_hermes_setup(
        &[location("/home/test/inbox", LocationMode::Inbox, false)],
        offer(false),
    );
    press_start(&mut app, &config_dir);
    assert!(app.hermes.pending_hermes_start.is_none());
    assert!(!app.hermes.hermes_setup.as_ref().unwrap().is_busy());
}

#[test]
fn start_does_nothing_for_folders_that_do_not_go_together() {
    let config_dir = TempDir::new().unwrap();
    let mut app = App::with_hermes_setup(
        &[
            location("/home/test/same", LocationMode::Inbox, false),
            location("/home/test/same", LocationMode::Archive, true),
        ],
        offer(false),
    );
    assert!(!app.hermes.hermes_setup.as_ref().unwrap().can_start());
    press_start(&mut app, &config_dir);
    assert!(app.hermes.pending_hermes_start.is_none());
}

#[test]
fn start_without_a_config_file_says_so_and_stays_free() {
    let index_dir = TempDir::new().unwrap();
    let mut app = app_at_the_question(&index_dir);
    drop(app.update(Message::HermesSetup(HermesSetupMessage::Start)));
    let setup = app.hermes.hermes_setup.as_ref().unwrap();
    assert_eq!(setup.note(), Some(&SetupNote::NoConfigPath));
    assert!(!setup.is_busy());
    assert!(app.hermes.pending_hermes_start.is_none());
}

#[test]
fn start_is_busy_while_the_locations_are_written() {
    let index_dir = TempDir::new().unwrap();
    let runtime = runtime();
    let _entered = runtime.enter();
    let config_dir = TempDir::new().unwrap();
    let mut app = app_at_the_question(&index_dir);
    press_start(&mut app, &config_dir);
    assert!(app.hermes.pending_hermes_start.is_some());
    assert!(app.hermes.setup_is_busy());
    assert!(!app.hermes.setup_focus_order().contains(&SetupFocus::Start));
}

#[test]
fn a_failed_write_frees_the_card_and_says_why() {
    let index_dir = TempDir::new().unwrap();
    let runtime = runtime();
    let _entered = runtime.enter();
    let config_dir = TempDir::new().unwrap();
    let mut app = app_at_the_question(&index_dir);
    press_start(&mut app, &config_dir);
    let error = FilingWriteError::Io {
        path: PathBuf::from("config.toml"),
        reason: "read-only".to_owned(),
    };
    drop(
        app.update(Message::HermesSetup(HermesSetupMessage::Written(Err(
            error,
        )))),
    );
    assert!(app.hermes.pending_hermes_start.is_none());
    let setup = app.hermes.hermes_setup.as_ref().unwrap();
    assert!(matches!(setup.note(), Some(SetupNote::SaveFailed(_))));
    assert!(!setup.is_busy());
}

#[test]
fn start_at_the_question_creates_the_index_with_the_choice() {
    let runtime = runtime();
    let _entered = runtime.enter();
    for encrypted in [true, false] {
        let config_dir = TempDir::new().unwrap();
        let index_dir = TempDir::new().unwrap();
        let mut app = app_at_the_question(&index_dir);
        drop(
            app.update(Message::HermesSetup(HermesSetupMessage::ChooseEncrypted(
                encrypted,
            ))),
        );
        press_start(&mut app, &config_dir);
        drop(
            app.update(Message::HermesSetup(HermesSetupMessage::Written(Ok(
                filing_with(chosen_folders()),
            )))),
        );
        assert!(
            matches!(
                app.hermes.filing.as_ref().unwrap().phase(),
                Phase::AskEncryption { encrypted: chosen, busy: true, .. } if *chosen == encrypted
            ),
            "encrypted = {encrypted}"
        );
        assert!(
            app.hermes.hermes_setup.is_some(),
            "the card stays while it works"
        );
        assert!(app.hermes.setup_is_busy());
    }
}

#[test]
fn a_remembered_choice_is_applied_when_the_module_asks_later() {
    let runtime = runtime();
    let _entered = runtime.enter();
    let mut app = App::with_hermes_setup(&chosen_folders(), offer(false));
    let setup = app.hermes.hermes_setup.as_mut().unwrap();
    setup.choose_encrypted(false);
    app.hermes.pending_hermes_start = Some(setup.start().unwrap().plan);
    app.hermes.filing = Some(FilingModule::in_phase(Phase::Starting));
    drop(app.advance_hermes_plan());
    assert!(matches!(
        app.hermes.filing.as_ref().unwrap().phase(),
        Phase::Starting
    ));

    let index_dir = TempDir::new().unwrap();
    app.hermes.filing = Some(FilingModule::in_phase_with_locations_at(
        asks_encryption(),
        index_dir.path().join("index.db"),
    ));
    drop(app.advance_hermes_plan());
    assert!(matches!(
        app.hermes.filing.as_ref().unwrap().phase(),
        Phase::AskEncryption {
            encrypted: false,
            busy: true,
            ..
        }
    ));
}

#[test]
fn the_card_gives_way_to_the_recovery_key() {
    let mut app = App::with_hermes_setup(&chosen_folders(), offer(false));
    let setup = app.hermes.hermes_setup.as_mut().unwrap();
    app.hermes.pending_hermes_start = Some(setup.start().unwrap().plan);
    app.hermes.filing = Some(FilingModule::in_phase(Phase::ShowRecoveryKey {
        key: FilingModule::fixed_recovery_key(),
        group: 3,
        typed: crate::filing::SecretText::default(),
        mismatch: false,
    }));
    drop(app.advance_hermes_plan());
    assert!(app.hermes.hermes_setup.is_none());
    assert!(
        app.hermes.pending_hermes_start.is_some(),
        "the plan waits for Ready"
    );
}

fn app_ready_after_start(load_model: bool, models_dir: &TempDir) -> App {
    let mut app = App::with_hermes_setup(&chosen_folders(), offer(true));
    app.hermes.index_path = Some(models_dir.path().join("index.db"));
    let setup = app.hermes.hermes_setup.as_mut().unwrap();
    if load_model {
        setup.toggle_load_model();
    }
    app.hermes.pending_hermes_start = Some(setup.start().unwrap().plan);
    app.hermes.filing = Some(FilingModule::in_phase(Phase::Ready));
    app
}

#[test]
fn the_model_is_loaded_after_ready_when_the_box_was_ticked() {
    let runtime = runtime();
    let _entered = runtime.enter();
    let models_dir = TempDir::new().unwrap();
    let mut app = app_ready_after_start(true, &models_dir);
    drop(app.advance_hermes_plan());
    assert!(app.hermes.hermes_setup.is_none());
    assert!(app.hermes.pending_hermes_start.is_none());
    assert!(app.hermes.ai_model_asked);
    assert!(app.settings.ai_model.as_ref().unwrap().is_downloading());
}

#[test]
fn the_model_is_left_alone_when_the_box_was_not_ticked() {
    let models_dir = TempDir::new().unwrap();
    let mut app = app_ready_after_start(false, &models_dir);
    drop(app.advance_hermes_plan());
    assert!(app.hermes.hermes_setup.is_none());
    assert!(app.settings.ai_model.is_none());
    assert!(app.hermes.ai_model_asked, "the module must not ask again");
}

#[test]
fn later_dismisses_the_module_and_closes_the_card() {
    let index_dir = TempDir::new().unwrap();
    let mut app = app_at_the_question(&index_dir);
    drop(app.update(Message::HermesSetup(HermesSetupMessage::Later)));
    assert!(app.hermes.hermes_setup.is_none());
    assert!(matches!(
        app.hermes.filing.as_ref().unwrap().phase(),
        Phase::Dismissed
    ));
}

#[test]
fn escape_is_later_and_enter_starts_when_it_can() {
    let index_dir = TempDir::new().unwrap();
    let runtime = runtime();
    let _entered = runtime.enter();
    let mut app = app_at_the_question(&index_dir);
    let state = app.prompt_key_state();
    assert!(state.hermes_setup);
    assert_eq!(
        route_key(&state, Key::Named(Named::Escape), Modifiers::default()),
        Some(Message::HermesSetup(HermesSetupMessage::Later))
    );
    assert_eq!(
        route_key(&state, Key::Named(Named::Enter), Modifiers::default()),
        Some(Message::DialogActivate)
    );
    assert_eq!(
        app.hermes.setup_focus_order().get(app.dialog_focus),
        Some(&SetupFocus::Start)
    );
    drop(app.update(Message::DialogActivate));
    assert_eq!(
        app.hermes.hermes_setup.as_ref().unwrap().note(),
        Some(&SetupNote::NoConfigPath),
        "Start was pressed, and the test app has no config file"
    );
}

#[test]
fn tab_walks_through_the_controls_and_wraps() {
    let mut app = App::with_hermes_setup(&[], offer(true));
    let order = app.hermes.setup_focus_order();
    assert_eq!(order.first(), Some(&SetupFocus::ChooseInbox));
    assert_eq!(app.dialog_focus, 0);
    for _ in 0..order.len() {
        drop(app.update(Message::DialogFocus(1)));
    }
    assert_eq!(app.dialog_focus, 0);
    drop(app.update(Message::DialogFocus(-1)));
    assert_eq!(app.dialog_focus, order.len() - 1);
}

#[test]
fn the_flow_moves_only_while_the_card_is_visible() {
    let mut app = App::with_fixed_panels();
    assert!(!app.hermes_flow_is_visible());
    app.hermes.hermes_setup = Some(HermesSetup::new(&[], offer(false)));
    assert!(app.hermes_flow_is_visible());
    drop(app.update(Message::HermesFlowTick));
    assert_eq!(app.hermes_flow_step, 1);
    app.hermes.hermes_setup = None;
    assert!(!app.hermes_flow_is_visible());
}

#[test]
fn a_module_that_asks_for_the_encryption_opens_the_card() {
    let mut app = App::with_fixed_panels();
    app.hermes.filing = Some(FilingModule::in_phase(asks_encryption()));
    drop(app.update(Message::Filing(FilingMessage::ChooseEncrypted(true))));
    assert!(app.hermes.hermes_setup.is_some());
}

#[test]
fn the_setup_command_opens_the_new_card_without_locations_and_the_old_one_with() {
    let runtime = runtime();
    let _entered = runtime.enter();
    let mut empty = App::with_fixed_panels();
    drop(empty.update(Message::FilingSetup(FilingSetupMessage::Toggle)));
    assert!(empty.hermes.hermes_setup.is_some() && empty.hermes.filing_setup.is_none());
    drop(empty.update(Message::FilingSetup(FilingSetupMessage::Toggle)));
    assert!(empty.hermes.hermes_setup.is_none());

    let mut configured = App::with_fixed_panels();
    configured.hermes.filing_config = filing_with(chosen_folders());
    configured.hermes.filing = Some(FilingModule::in_phase_with_locations(Phase::Ready));
    drop(configured.update(Message::FilingSetup(FilingSetupMessage::Toggle)));
    assert!(configured.hermes.filing_setup.is_some() && configured.hermes.hermes_setup.is_none());
}
