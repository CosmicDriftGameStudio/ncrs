use crate::app::*;
use crate::palette::ThemePreset;
use crate::vsix::fixtures::{manifest, package, THEME_JSON};

const CONFIG: &str = "/example/config.toml";

fn extension(themes: &[(&str, &str, &str)]) -> FetchedExtension {
    let manifest = manifest(themes);
    let vsix = package(&[
        ("package.json", manifest.as_bytes()),
        ("themes/dark.json", THEME_JSON.as_bytes()),
        ("themes/light.json", THEME_JSON.as_bytes()),
    ]);
    FetchedExtension {
        id: ExtensionId::parse("example.dark-themes").unwrap(),
        display_name: "Example Themes".to_owned(),
        version: "1.2.3".to_owned(),
        license: Some("MIT".to_owned()),
        vsix: vsix.into(),
    }
}

fn two_themes(_: &ExtensionId) -> Result<FetchedExtension, FetchError> {
    Ok(extension(&[
        ("Example Dark", "vs-dark", "./themes/dark.json"),
        ("Example Light", "vs", "./themes/light.json"),
    ]))
}

fn one_theme(_: &ExtensionId) -> Result<FetchedExtension, FetchError> {
    Ok(extension(&[(
        "Example Dark",
        "vs-dark",
        "./themes/dark.json",
    )]))
}

fn not_found(id: &ExtensionId) -> Result<FetchedExtension, FetchError> {
    Err(FetchError::NotFound(id.to_string()))
}

/// What the real install would hand back, without a file anywhere.
fn installed(pick: &Pick, _: &Path) -> Result<Box<Installed>, FetchError> {
    Ok(Box::new(Installed {
        config: Config {
            palette: ThemePreset::Classic.palette(),
            ..Config::default()
        },
        label: pick.1.label.clone(),
        license: pick.0.extension.license.clone(),
    }))
}

fn warned(pick: &Pick, config_path: &Path) -> Result<Box<Installed>, FetchError> {
    let mut result = installed(pick, config_path)?;
    result.config.vscode_warning = Some(VscodeWarning {
        path: config_path.to_path_buf(),
        line: None,
        theme: PathBuf::from("themes/dark.json"),
        reason: "no colors".to_owned(),
    });
    Ok(result)
}

fn fetch_message(message: ThemeFetchMessage) -> Message {
    Message::ThemeFetch(message)
}

fn app_fetching_with(fetch: fn(&ExtensionId) -> Result<FetchedExtension, FetchError>) -> App {
    let mut app = App::with_fixed_panels();
    app.config_path = Some(PathBuf::from(CONFIG));
    app.fetch_extension = fetch;
    app.install_theme = installed;
    app
}

fn send(app: &mut App, message: ThemeFetchMessage) {
    drop(app.update(fetch_message(message)));
}

fn type_id(app: &mut App) {
    send(app, ThemeFetchMessage::Open);
    send(
        app,
        ThemeFetchMessage::TextChanged("example.dark-themes".to_owned()),
    );
}

/// Hands the card the answer of the download it started, the way the
/// task does.
fn deliver_download(app: &mut App) -> Result<Fetched, FetchError> {
    let id = ExtensionId::parse(app.theme_fetch_for_test().unwrap().text()).unwrap();
    let result = download_theme(app.fetch_extension, &id);
    let generation = app.settings.theme_fetch_generation;
    send(
        app,
        ThemeFetchMessage::Fetched {
            generation,
            result: result.clone(),
        },
    );
    result
}

fn deliver_install(app: &mut App, fetched: &Fetched, index: usize) {
    let pick = (fetched.clone(), fetched.themes[index].clone());
    let result = (app.install_theme)(&pick, Path::new(CONFIG));
    let generation = app.settings.theme_fetch_generation;
    send(app, ThemeFetchMessage::Installed { generation, result });
}

fn stage(app: &App) -> &Stage {
    app.theme_fetch_for_test().unwrap().stage()
}

#[test]
fn the_palette_entry_opens_the_card_and_it_takes_the_keys() {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::CommandPalette(CommandPaletteMessage::Toggle)));
    drop(app.update(Message::CommandPalette(
        CommandPaletteMessage::QueryChanged("color theme".to_owned()),
    )));
    drop(app.update(Message::CommandPalette(CommandPaletteMessage::Run)));

    assert!(app.command_palette_for_test().is_none());
    assert_eq!(stage(&app), &Stage::Input);
    assert!(app.prompt_key_state().theme_fetch);
    assert!(app.blocking_overlay_is_open());
}

#[test]
fn it_is_not_opened_over_another_modal() {
    let mut app = App::with_fixed_panels();
    drop(app.update(Message::CommandPalette(CommandPaletteMessage::Toggle)));
    send(&mut app, ThemeFetchMessage::Open);
    assert!(app.theme_fetch_for_test().is_none());
}

#[tokio::test]
async fn two_themes_are_listed_and_the_picked_one_is_installed() {
    let mut app = app_fetching_with(two_themes);
    let before = app.palette;
    type_id(&mut app);
    send(&mut app, ThemeFetchMessage::Activate);
    assert_eq!(stage(&app), &Stage::Fetching);

    let fetched = deliver_download(&mut app).unwrap();
    let Stage::Choosing {
        fetched: offered,
        cursor,
    } = stage(&app)
    else {
        panic!("not choosing: {:?}", stage(&app));
    };
    let labels: Vec<_> = offered.themes.iter().map(|t| t.label.as_str()).collect();
    assert_eq!(labels, ["Example Dark", "Example Light"]);
    assert_eq!(*cursor, 0);

    send(&mut app, ThemeFetchMessage::Move(1));
    send(&mut app, ThemeFetchMessage::Activate);
    assert_eq!(stage(&app), &Stage::Installing);

    deliver_install(&mut app, &fetched, 1);
    assert!(app.theme_fetch_for_test().is_none(), "the card closes");
    assert_ne!(app.palette, before, "the look changes at once");
    assert_eq!(app.palette, ThemePreset::Classic.palette());
    assert_eq!(
        app.notice_for_test(),
        Some("Color theme \"Example Light\" installed from Open VSX (license: MIT).")
    );
    assert_eq!(app.config_error, None);
}

#[tokio::test]
async fn a_click_on_a_theme_installs_it() {
    let mut app = app_fetching_with(two_themes);
    type_id(&mut app);
    send(&mut app, ThemeFetchMessage::Activate);
    deliver_download(&mut app).unwrap();

    send(&mut app, ThemeFetchMessage::Pick(1));

    assert_eq!(stage(&app), &Stage::Installing);
}

#[tokio::test]
async fn a_single_theme_is_installed_without_asking() {
    let mut app = app_fetching_with(one_theme);
    type_id(&mut app);
    send(&mut app, ThemeFetchMessage::Activate);

    let fetched = deliver_download(&mut app).unwrap();
    assert_eq!(stage(&app), &Stage::Installing);

    deliver_install(&mut app, &fetched, 0);
    assert!(app.theme_fetch_for_test().is_none());
    assert!(app
        .notice_for_test()
        .is_some_and(|notice| notice.contains("Example Dark")));
}

#[tokio::test]
async fn a_failed_download_keeps_the_input_and_says_why() {
    let mut app = app_fetching_with(not_found);
    type_id(&mut app);
    send(&mut app, ThemeFetchMessage::Activate);

    deliver_download(&mut app).unwrap_err();

    let card = app.theme_fetch_for_test().unwrap();
    assert_eq!(card.stage(), &Stage::Input);
    assert_eq!(card.text(), "example.dark-themes");
    assert_eq!(
        card.error(),
        Some(&FetchError::NotFound("example.dark-themes".to_owned()))
    );
}

#[test]
fn a_text_that_names_no_extension_fetches_nothing() {
    let mut app = app_fetching_with(two_themes);
    send(&mut app, ThemeFetchMessage::Open);
    send(
        &mut app,
        ThemeFetchMessage::TextChanged("../etc".to_owned()),
    );
    send(&mut app, ThemeFetchMessage::Activate);

    let card = app.theme_fetch_for_test().unwrap();
    assert_eq!(card.stage(), &Stage::Input);
    assert_eq!(card.error(), Some(&FetchError::InvalidId));
}

#[tokio::test]
async fn a_late_answer_for_a_closed_card_is_dropped() {
    let mut app = app_fetching_with(two_themes);
    type_id(&mut app);
    send(&mut app, ThemeFetchMessage::Activate);
    let generation = app.settings.theme_fetch_generation;
    let result = download_theme(
        two_themes,
        &ExtensionId::parse("example.dark-themes").unwrap(),
    );

    let routed = route_key(
        &app.prompt_key_state(),
        Key::Named(Named::Escape),
        Modifiers::default(),
    );
    assert_eq!(routed, Some(fetch_message(ThemeFetchMessage::Close)));
    drop(app.update(routed.unwrap()));
    assert!(app.theme_fetch_for_test().is_none());

    send(
        &mut app,
        ThemeFetchMessage::Fetched {
            generation,
            result: result.clone(),
        },
    );
    assert!(app.theme_fetch_for_test().is_none(), "nothing reopens");

    // A card opened since is not the one the answer was for.
    send(&mut app, ThemeFetchMessage::Open);
    send(&mut app, ThemeFetchMessage::Fetched { generation, result });
    assert_eq!(stage(&app), &Stage::Input);
}

#[tokio::test]
async fn closing_waits_while_the_theme_is_written() {
    let mut app = app_fetching_with(one_theme);
    type_id(&mut app);
    send(&mut app, ThemeFetchMessage::Activate);
    let fetched = deliver_download(&mut app).unwrap();
    assert_eq!(stage(&app), &Stage::Installing);

    send(&mut app, ThemeFetchMessage::Close);
    assert_eq!(stage(&app), &Stage::Installing);

    deliver_install(&mut app, &fetched, 0);
    assert!(app.theme_fetch_for_test().is_none());
}

#[tokio::test]
async fn without_a_config_file_the_theme_cannot_be_installed() {
    let mut app = app_fetching_with(one_theme);
    app.config_path = None;
    type_id(&mut app);
    send(&mut app, ThemeFetchMessage::Activate);

    deliver_download(&mut app).unwrap();

    let card = app.theme_fetch_for_test().unwrap();
    assert_eq!(card.stage(), &Stage::Input);
    assert_eq!(card.error(), Some(&FetchError::NoConfigPath));
}

#[tokio::test]
async fn a_failed_installation_returns_to_the_input() {
    let mut app = app_fetching_with(one_theme);
    let before = app.palette;
    type_id(&mut app);
    send(&mut app, ThemeFetchMessage::Activate);
    deliver_download(&mut app).unwrap();
    let generation = app.settings.theme_fetch_generation;
    let error = FetchError::WriteFailed {
        path: CONFIG.to_owned(),
        reason: "disk full".to_owned(),
    };

    send(
        &mut app,
        ThemeFetchMessage::Installed {
            generation,
            result: Err(error.clone()),
        },
    );

    let card = app.theme_fetch_for_test().unwrap();
    assert_eq!(card.stage(), &Stage::Input);
    assert_eq!(card.error(), Some(&error));
    assert_eq!(card.text(), "example.dark-themes");
    assert_eq!(app.palette, before);
}

#[tokio::test]
async fn a_theme_that_was_not_applied_in_full_is_reported_like_a_config_does() {
    let mut app = app_fetching_with(one_theme);
    app.install_theme = warned;
    type_id(&mut app);
    send(&mut app, ThemeFetchMessage::Activate);
    let fetched = deliver_download(&mut app).unwrap();

    deliver_install(&mut app, &fetched, 0);

    assert!(app
        .config_error
        .as_deref()
        .is_some_and(|text| text.contains("no colors")));
    assert!(app.notice_for_test().is_some());
}

#[tokio::test]
async fn the_warning_of_the_look_before_goes_away_with_a_clean_install() {
    let mut app = app_fetching_with(one_theme);
    app.report(ConfigProblem::VscodeTheme(VscodeWarning {
        path: PathBuf::from(CONFIG),
        line: None,
        theme: PathBuf::from("old.json"),
        reason: "unreadable".to_owned(),
    }));
    assert!(app.config_error.is_some());
    type_id(&mut app);
    send(&mut app, ThemeFetchMessage::Activate);
    let fetched = deliver_download(&mut app).unwrap();

    deliver_install(&mut app, &fetched, 0);

    assert_eq!(app.config_error, None);
}

#[tokio::test]
async fn an_unreadable_config_error_stays_when_a_theme_is_installed() {
    let mut app = app_fetching_with(one_theme);
    app.report(ConfigProblem::Invalid(ConfigError::Unreadable {
        path: PathBuf::from(CONFIG),
        reason: "denied".to_owned(),
    }));
    type_id(&mut app);
    send(&mut app, ThemeFetchMessage::Activate);
    let fetched = deliver_download(&mut app).unwrap();

    deliver_install(&mut app, &fetched, 0);

    assert!(app.config_error.is_some());
}

#[test]
fn the_keys_are_the_cards_while_it_is_up() {
    let mut app = App::with_fixed_panels();
    send(&mut app, ThemeFetchMessage::Open);
    let route = |key: Named| {
        route_key(
            &app.prompt_key_state(),
            Key::Named(key),
            Modifiers::default(),
        )
    };
    assert_eq!(
        route(Named::Enter),
        Some(fetch_message(ThemeFetchMessage::Activate))
    );
    assert_eq!(
        route(Named::ArrowDown),
        Some(fetch_message(ThemeFetchMessage::Move(1)))
    );
    assert_eq!(
        route(Named::ArrowUp),
        Some(fetch_message(ThemeFetchMessage::Move(-1)))
    );
    assert_eq!(
        route(Named::Escape),
        Some(fetch_message(ThemeFetchMessage::Close))
    );
    for dead in [Named::F5, Named::F7, Named::F8, Named::Backspace] {
        assert_eq!(route(dead), None, "{dead:?}");
    }
    assert_eq!(route(Named::F10), Some(Message::Quit));
}

#[test]
fn the_input_state_closes_it_on_escape_too() {
    let mut input = InputState {
        theme_fetch: true,
        job_running: true,
        ..InputState::default()
    };
    let escape = iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key: Key::Named(Named::Escape),
        modified_key: Key::Named(Named::Escape),
        physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Escape),
        location: keyboard::Location::Standard,
        modifiers: Modifiers::default(),
        repeat: false,
        text: None,
    });
    assert_eq!(
        input.handle(&escape, event::Status::Ignored),
        Some(fetch_message(ThemeFetchMessage::Close))
    );
}

#[test]
fn holding_an_arrow_moves_on_but_enter_acts_once() {
    assert!(repeats(&fetch_message(ThemeFetchMessage::Move(1))));
    assert!(!repeats(&fetch_message(ThemeFetchMessage::Activate)));
}
