use crate::app::*;

fn invalid(line: Option<usize>) -> ConfigError {
    ConfigError::Invalid {
        path: PathBuf::from("/cfg/ncrs/config.toml"),
        line,
        reason: "unknown field `colour`".to_string(),
    }
}

#[test]
fn an_error_with_a_line_names_path_line_and_reason() {
    let app = App::with_fixed_panels().with_config(Err(invalid(Some(3))));
    let shown = app.status_error().unwrap_or_default();
    assert!(shown.contains("/cfg/ncrs/config.toml"), "{shown}");
    assert!(shown.contains("line 3"), "{shown}");
    assert!(shown.contains("unknown field `colour`"), "{shown}");
}

#[test]
fn an_unreadable_file_names_path_and_reason() {
    let err = ConfigError::Unreadable {
        path: PathBuf::from("/cfg/ncrs/config.toml"),
        reason: "not a regular file".to_string(),
    };
    let app = App::with_fixed_panels().with_config(Err(err));
    let shown = app.status_error().unwrap_or_default();
    assert!(shown.contains("/cfg/ncrs/config.toml"), "{shown}");
    assert!(shown.contains("not a regular file"), "{shown}");
}

#[test]
fn an_error_without_a_line_still_reports() {
    let app = App::with_fixed_panels().with_config(Err(invalid(None)));
    let shown = app.status_error().unwrap_or_default();
    assert!(shown.contains("unknown field"), "{shown}");
    assert!(!shown.contains("line"), "{shown}");
}

#[test]
fn a_good_config_shows_nothing_and_is_applied() {
    let mut config = Config::default();
    config.open.view = Some(config::ProgramLine::new("viewer", &[]));
    let app = App::with_fixed_panels().with_config(Ok(config.clone()));
    assert_eq!(app.status_error(), None);
    assert_eq!(app.open_programs, config.open);
}

#[test]
fn a_config_error_is_its_own_field_and_shows_in_the_status_line() {
    let app = App::with_fixed_panels().with_config(Err(invalid(Some(3))));
    assert_eq!(app.job_error, None);
    assert!(app.config_error.is_some());
    assert_eq!(app.status_error(), app.config_error.as_deref());
}

#[test]
fn a_job_error_covers_the_config_error_until_it_goes() {
    let mut app = App::with_fixed_panels().with_config(Err(invalid(Some(3))));
    let config_text = app.config_error.clone();
    app.set_status_error("x");
    assert_eq!(app.status_error(), Some("x"));

    drop(app.update(Message::Panel(PanelMessage::MoveSelection(1))));

    assert_eq!(app.status_error(), config_text.as_deref());
}

#[test]
fn escape_dismisses_the_config_error_and_a_job_comes_first() {
    let escape = Key::Named(Named::Escape);
    let state = |job_running, config_error| PromptKeyState {
        job_running,
        config_error,
        ..PromptKeyState::default()
    };
    assert_eq!(
        route_key(&state(false, true), escape.clone(), Modifiers::default()),
        Some(Message::DismissConfigError)
    );
    assert_eq!(
        route_key(&state(true, true), escape.clone(), Modifiers::default()),
        Some(Message::AbortJob)
    );
    assert_eq!(
        route_key(&state(false, false), escape, Modifiers::default()),
        None
    );
}

#[test]
fn the_config_error_follows_a_language_switch() {
    let mut app = App::with_fixed_panels().with_config(Err(invalid(Some(3))));
    drop(app.update(Message::SwitchLanguage));
    let shown = app.status_error().unwrap_or_default();
    assert!(shown.contains("Zeile 3"), "{shown}");
}

fn loaded(content: &str) -> Config {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, content).unwrap();
    config::load(&path).unwrap()
}

#[test]
fn a_vscode_theme_that_is_not_found_is_reported_in_both_languages_and_beats_a_contrast_warning() {
    let mut app = App::with_fixed_panels().with_config(Ok(loaded(
        "lang = \"de\"\n[theme]\npanel = \"#f4f0ff\"\nvscode = \"gone.json\"\n",
    )));
    let german = app.status_error().unwrap_or_default();
    assert!(german.contains("Zeile 4"), "{german}");
    assert!(german.contains("VS-Code-Theme"), "{german}");
    assert!(german.contains("gone.json: no such file"), "{german}");
    assert!(!german.contains("Kontrast"), "{german}");

    drop(app.update(Message::SwitchLanguage));
    let english = app.status_error().unwrap_or_default();
    assert!(english.contains("line 4"), "{english}");
    assert!(english.contains("VS Code theme"), "{english}");
    assert!(english.contains("the preset applies"), "{english}");
}

#[test]
fn an_unknown_language_is_reported_in_both_languages_and_english_is_used() {
    let mut app = App::with_fixed_panels().with_config(Ok(loaded("\nlang = \"fr\"\n")));
    assert_eq!(app.lang, Language::English);
    let english = app.status_error().unwrap_or_default();
    assert!(english.contains("line 2"), "{english}");
    assert!(english.contains("unknown language \"fr\""), "{english}");
    assert!(
        english.contains("\"en\", \"de\", \"en-futurama\", \"de-futurama\""),
        "{english}"
    );

    drop(app.update(Message::SwitchLanguage));
    let german = app.status_error().unwrap_or_default();
    assert!(german.contains("Zeile 2"), "{german}");
    assert!(german.contains("unbekannte Sprache \"fr\""), "{german}");
}

#[test]
fn skipped_kinds_are_named_in_both_languages_and_cleared_by_a_fresh_look() {
    let text = "[[kinds]]\nid = \"\"\n[[kinds]]\nid = \"payslip\"\nwords = [\"Gehaltsabrechnung\"]\n[[kinds]]\nid = \"wage\"\nwords = [\"Gehaltsabrechnung\"]\n";
    let mut app = App::with_fixed_panels().with_config(Ok(loaded(text)));
    let english = app.status_error().unwrap_or_default();
    assert!(english.contains("an entry without an id"), "{english}");
    assert!(
        english.contains("\"Gehaltsabrechnung\" in wage"),
        "{english}"
    );
    assert_eq!(
        app.kinds.catalog.stored_kind("Gehaltsabrechnung"),
        "payslip"
    );

    drop(app.update(Message::SwitchLanguage));
    let german = app.status_error().unwrap_or_default();
    assert!(german.contains("ein Eintrag ohne id"), "{german}");
    assert!(german.contains("„Gehaltsabrechnung“ bei wage"), "{german}");

    app.apply_look(loaded(""));
    assert_eq!(app.status_error(), None);
    assert_eq!(
        app.kinds.catalog.stored_kind("Gehaltsabrechnung"),
        "Gehaltsabrechnung"
    );
}

#[test]
fn a_configured_color_reaches_the_app() {
    let app = App::with_fixed_panels().with_config(Ok(loaded("[theme]\nerror = \"#f0a0a0\"\n")));
    assert_eq!(app.palette.error, iced::Color::from_rgb8(0xF0, 0xA0, 0xA0));
    assert_eq!(app.status_error(), None);
}

#[test]
fn the_configured_start_language_words_the_first_message() {
    let app = App::with_fixed_panels()
        .with_config(Ok(loaded("lang = \"de\"\n[theme]\npanel = \"#f4f0ff\"\n")));
    assert_eq!(app.lang, Language::German);
    let shown = app.status_error().unwrap_or_default();
    assert!(shown.contains("Zeile 3"), "{shown}");
    let english = App::with_fixed_panels().with_config(Ok(loaded("")));
    assert_eq!(english.lang, Language::English);
}

#[test]
fn a_low_contrast_is_shown_follows_the_language_and_is_dismissed() {
    let mut app = App::with_fixed_panels().with_config(Ok(loaded(
        "[theme]\npanel = \"#f4f0ff\"\npreset = \"cosmic\"\n",
    )));
    assert_eq!(app.palette.panel, iced::Color::from_rgb8(0xF4, 0xF0, 0xFF));
    let shown = app.status_error().unwrap_or_default();
    assert!(shown.contains("line 2"), "{shown}");
    assert!(
        shown.contains("directory on panel has a contrast of only"),
        "{shown}"
    );
    assert!(!shown.contains("Using the defaults"), "{shown}");

    drop(app.update(Message::SwitchLanguage));
    let german = app.status_error().unwrap_or_default();
    assert!(german.contains("Zeile 2"), "{german}");
    assert!(
        german.contains("directory auf panel hat nur ein Kontrastverhältnis"),
        "{german}"
    );

    drop(app.update(Message::DismissConfigError));
    assert_eq!(app.status_error(), None);
    assert_eq!(app.palette.panel, iced::Color::from_rgb8(0xF4, 0xF0, 0xFF));
}

#[test]
fn dismissing_clears_the_config_error() {
    let mut app = App::with_fixed_panels().with_config(Err(invalid(Some(3))));
    drop(app.update(Message::DismissConfigError));
    assert_eq!(app.status_error(), None);
}
