use crate::app::*;
use crate::messages::{SettingField, SettingsFeature};
use crate::palette::ColorRole;

fn open(app: &mut App) {
    drop(app.update(Message::Settings(SettingsMessage::Toggle)));
}

fn press(app: &App, key: Key, modifiers: Modifiers) -> Option<Message> {
    route_key(&app.prompt_key_state(), key, modifiers)
}

#[test]
fn the_key_opens_the_tab_and_closes_it_again() {
    let mut app = App::with_fixed_panels();
    open(&mut app);
    assert!(app.settings.page.is_some());
    open(&mut app);
    assert!(app.settings.page.is_none());
}

#[test]
fn escape_and_the_panel_tabs_close_it() {
    let mut app = App::with_fixed_panels();
    open(&mut app);
    assert_eq!(
        press(&app, Key::Named(Named::Escape), Modifiers::empty()),
        Some(Message::Settings(SettingsMessage::Close))
    );
    drop(app.update(Message::Panel(PanelMessage::ActivatePanel(
        PanelSide::Right,
    ))));
    assert!(app.settings.page.is_none());
}

#[test]
fn the_panel_keys_do_nothing_while_it_is_open() {
    let mut app = App::with_fixed_panels();
    open(&mut app);
    assert_eq!(
        press(&app, Key::Named(Named::ArrowDown), Modifiers::empty()),
        None
    );
}

#[test]
fn a_change_without_a_config_place_is_reported_and_applies_nothing() {
    let mut app = App::with_fixed_panels();
    app.config_path = None;
    open(&mut app);
    drop(app.update(Message::Settings(SettingsMessage::Change(
        SettingChange::Language(Language::German),
    ))));
    assert!(app.settings.failure.is_some());
    assert_eq!(app.settings.page.as_ref().unwrap().changed, 0);
}

#[test]
fn a_written_language_is_applied_and_counted() {
    let mut app = App::with_fixed_panels().in_language(Language::English);
    open(&mut app);
    drop(app.update(Message::Settings(SettingsMessage::Written {
        change: SettingChange::Language(Language::German),
        result: Ok(Box::default()),
    })));
    assert_eq!(app.lang, Language::German);
    assert_eq!(app.settings.page.as_ref().unwrap().changed, 1);
}

#[test]
fn a_failed_write_is_shown_and_changes_nothing() {
    let mut app = App::with_fixed_panels().in_language(Language::English);
    open(&mut app);
    drop(app.update(Message::Settings(SettingsMessage::Written {
        change: SettingChange::Language(Language::German),
        result: Err(FilingWriteError::Io {
            path: PathBuf::from("/x/config.toml"),
            reason: "denied".to_owned(),
        }),
    })));
    assert_eq!(app.lang, Language::English);
    assert!(app.settings.failure.as_deref().unwrap().contains("denied"));
}

#[test]
fn a_written_program_takes_effect_for_the_next_f3() {
    let mut app = App::with_settings_file(SettingsFeature::Programs, Ok(Config::default()));
    let line = config::ProgramLine::new("bat", &[]);
    let change = SettingChange::Program {
        kind: crate::fs::OpenKind::View,
        line: Some(line.clone()),
    };
    let written = config::from_text("[open]\nview = [\"bat\"]\n").unwrap();
    drop(app.update(Message::Settings(SettingsMessage::Written {
        change,
        result: Ok(Box::new(written)),
    })));
    assert_eq!(app.open_programs.view, Some(line));
}

#[test]
fn a_written_color_is_painted_at_once() {
    let mut app = App::with_settings_file(SettingsFeature::Look, Ok(Config::default()));
    let written = config::from_text("[theme]\naccent = \"#ff0000\"\n").unwrap();
    drop(app.update(Message::Settings(SettingsMessage::Written {
        change: SettingChange::Color {
            role: ColorRole::Accent,
            color: Some("#ff0000".to_owned()),
        },
        result: Ok(Box::new(written)),
    })));
    assert_eq!(
        app.palette.color(ColorRole::Accent),
        iced::Color::from_rgb8(0xFF, 0, 0)
    );
}

#[test]
fn a_refused_field_value_is_shown_at_its_row_and_not_in_the_status_bar() {
    let mut app = App::with_settings_file(SettingsFeature::Look, Ok(Config::default()));
    let field = SettingField::Color(ColorRole::Accent);
    drop(app.update(Message::Settings(SettingsMessage::Written {
        change: SettingChange::Color {
            role: ColorRole::Accent,
            color: Some("#ff0000".to_owned()),
        },
        result: Err(FilingWriteError::Io {
            path: PathBuf::from("/x/config.toml"),
            reason: "denied".to_owned(),
        }),
    })));
    assert!(app.settings.failure.is_none());
    let state = app.settings.page.as_ref().unwrap();
    assert!(
        matches!(state.problems.get(&field), Some(settings_ui::FieldProblem::Write(text)) if text.contains("denied"))
    );
}

fn typed_without_enter(app: &mut App, field: SettingField, text: &str) {
    drop(app.update(Message::Settings(SettingsMessage::Edit {
        field,
        text: text.to_owned(),
    })));
}

#[test]
fn a_wrong_value_typed_without_enter_keeps_the_page_and_the_tab() {
    let mut app = App::with_settings_file(SettingsFeature::Look, Ok(Config::default()));
    let accent = SettingField::Color(ColorRole::Accent);
    typed_without_enter(&mut app, accent, "red");

    drop(app.update(Message::Settings(SettingsMessage::Select(
        SettingsFeature::Language,
    ))));
    let state = app.settings.page.as_ref().unwrap();
    assert_eq!(state.feature, SettingsFeature::Look);
    assert_eq!(
        state.problems.get(&accent),
        Some(&settings_ui::FieldProblem::Color)
    );

    drop(app.update(Message::Settings(SettingsMessage::Close)));
    assert!(app.settings.page.is_some());
    assert_eq!(
        app.settings.page.as_ref().unwrap().field_text(accent),
        "red"
    );
}

#[test]
fn a_right_value_typed_without_enter_lets_the_page_change_and_the_tab_close() {
    let mut app = App::with_settings_file(SettingsFeature::Look, Ok(Config::default()));
    typed_without_enter(&mut app, SettingField::Color(ColorRole::Accent), "#ff0");
    drop(app.update(Message::Settings(SettingsMessage::Select(
        SettingsFeature::Language,
    ))));
    assert_eq!(
        app.settings.page.as_ref().unwrap().feature,
        SettingsFeature::Language
    );
    drop(app.update(Message::Settings(SettingsMessage::Close)));
    assert!(app.settings.page.is_none());
}

#[test]
fn a_write_that_fails_after_the_tab_closed_shows_in_the_status_bar() {
    let mut app = App::with_settings_file(SettingsFeature::Look, Ok(Config::default()));
    drop(app.update(Message::Settings(SettingsMessage::Close)));
    drop(app.update(Message::Settings(SettingsMessage::Written {
        change: SettingChange::Color {
            role: ColorRole::Accent,
            color: Some("#ff0000".to_owned()),
        },
        result: Err(FilingWriteError::Io {
            path: PathBuf::from("/x/config.toml"),
            reason: "denied".to_owned(),
        }),
    })));
    assert!(app.notice_for_test().unwrap().contains("denied"));
}

#[test]
fn a_theme_file_next_to_the_config_is_named_from_its_folder() {
    let config_path = Path::new("/home/me/.config/ncrs/config.toml");
    assert_eq!(
        theme_file_entry(
            Some(config_path),
            Path::new("/home/me/.config/ncrs/themes/night.json")
        ),
        Some("themes/night.json".to_owned())
    );
    assert_eq!(
        theme_file_entry(Some(config_path), Path::new("/opt/themes/day.json")),
        Some("/opt/themes/day.json".to_owned())
    );
}

#[test]
fn the_tab_opens_on_a_config_that_does_not_load_and_names_the_error() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("config.toml");
    std::fs::write(&path, "lang = 3\n").unwrap();
    let mut app = App::with_fixed_panels();
    app.config_path = Some(path);
    open(&mut app);
    let state = app.settings.page.as_ref().expect("the tab is open");
    assert_eq!(
        state.file.as_ref().err().and_then(ConfigError::line),
        Some(1)
    );
}

#[test]
fn a_page_is_selected_and_searched() {
    let mut app = App::with_fixed_panels();
    open(&mut app);
    drop(app.update(Message::Settings(SettingsMessage::Select(
        SettingsFeature::Hermes,
    ))));
    assert_eq!(
        app.settings.page.as_ref().unwrap().feature,
        SettingsFeature::Hermes
    );
}
